//! BB spec: effects/text-formations-text-fun-factory.md; common.md §1–§4, §5.1, §5.9–§5.11
//!
//! Text Fun Factory's letters (BB31): one outline per UTF-16 unit, placed at its whole raw
//! prefix's measured width plus k × Overlap (R158 Q68); mirrored, turned and jumbled by the letter
//! counters; filled solid, from a palette, with a per-letter gradient (Q70) or a canvas-anchored
//! texture brush; a centred outline under or over the fill; an always-applied shadow; each letter's
//! chain stacked under (or over) the ones before it, over transparent or the source. Each chain
//! runs in its own buffer: the letter's ink plus the shadow's and the bevel's reach, within the
//! layer plus that reach (R158, R159 halo), so geometry just off the layer still casts inward.
//! BB32: each letter's 3D bevel (also Silver and Copper's) and the Gold and Chrome faces, from
//! the letter's own coverage before its outline (R159). BB33: "Use Image on Clipboard" tiles the
//! session's clipboard image at its own size, or takes the Solid fill without one.
use super::bevel::relief_from_soft;
use super::blocks::{
    Shade, blur_soft, bokeh_with_edge, colour_exact, colour_raw, d2d_blend, drop_shadow_with,
    hsl_adjust, lighting, shadow_offset, source_over, straight_over, with_alpha,
};
use super::conventions::*;
use super::manifest::manifest;
use super::raster::{Cap, Join, Pen, widen};
use super::text::{coverage, layout_raw_units, settings};
use super::textures::{TEXTURE_NAMES, texture};
use super::ui::{
    ControlUi, External, Help, IndirectUi, Note, TRACK_BLACK_WHITE_BLACK, TRACK_HUE,
    TRACK_SATURATION, TRACK_WHITE_BLACK, Tab, TabStrip,
};
use crate::canvas::ImageData;
use crate::effect::blur::bokeh_len;
use crate::effect::distort::{catmull_rom, filter4};
use crate::effect::mix::mix;
use crate::effect::sample::gaussian_kernel;
use crate::effect::surface::{Alpha, Edge, EffectSurface, premultiply, unpremultiply};
use crate::effect::{
    ColorDefault, Control, EffectDef, EffectEnv, EffectKind, RenderInfo, Values, choice,
    color_value, flag, num, range,
};
use crate::fonts::FaceStyle;
use crate::pixel::gamma::{linear_to_srgb_f32, srgb_to_linear_f32};
use crate::pixel::{IntRect, Px};
use crate::tool::text::{DrawItem, TextLine, needs_color_font};
use serde_json::json;
use std::sync::LazyLock;
use tiny_skia::{Path, Rect, Transform};

static JUMBLE: LazyLock<String> =
    LazyLock::new(|| format!("\nEvens{}Jumble:{}Odds", " ".repeat(22), " ".repeat(21)));
static OVERLAP: LazyLock<String> = LazyLock::new(|| {
    format!(
        "\nOverlap{}Kerning:{}Spread",
        " ".repeat(18),
        " ".repeat(18)
    )
});
static TEXTURE_IDS: LazyLock<Vec<String>> =
    LazyLock::new(|| (0..TEXTURE_NAMES.len()).map(|i| format!("t{i}")).collect());
static TEXTURE_CHOICES: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    TEXTURE_IDS
        .iter()
        .map(String::as_str)
        .zip(TEXTURE_NAMES)
        .collect()
});

const NO: ControlUi = ControlUi::NO_HEADER;
const fn caption(caption: &'static str) -> ControlUi {
    ControlUi { caption, ..NO }
}
const NO_RESET: ControlUi = ControlUi { reset: false, ..NO };
const STRENGTH: ControlUi = ControlUi {
    decimals: Some(3),
    track: Some(&TRACK_WHITE_BLACK),
    ..NO
};
const ANGLE: ControlUi = ControlUi {
    decimals: Some(3),
    wrap: true,
    ..NO
};
const SWING: ControlUi = ControlUi {
    track: Some(&TRACK_BLACK_WHITE_BLACK),
    ..NO
};

static UI: IndirectUi = IndirectUi {
    width: 1.1,
    // BB33 / R155: only the effective Image fill on "Use Image on Clipboard" reads it.
    external_for: |v| {
        if clipboard_fill(v) {
            External::Clipboard
        } else {
            External::None
        }
    },
    tabs: &[
        TabStrip {
            key: "tab",
            tabs: &[
                Tab {
                    caption: "Text",
                    keys: &[
                        "text", "font", "bold", "italics", "size", "offsetX", "offsetY", "maintain",
                    ],
                },
                Tab {
                    caption: "Fill",
                    keys: &["fillTab"],
                },
                Tab {
                    caption: "Outline",
                    keys: &["outlineColor", "outlineSize", "corner", "outlineOnTop"],
                },
                Tab {
                    caption: "Shadow",
                    keys: &["shadowColor", "shadowLength", "shadowStrength", "light"],
                },
                Tab {
                    caption: "3D",
                    keys: &[
                        "depth",
                        "bevelStrength",
                        "highlight",
                        "hardBevel",
                        "overlayStyle",
                    ],
                },
                Tab {
                    caption: "Fun",
                    keys: &[
                        "rotate",
                        "rotateAngle",
                        "jumble",
                        "reverse",
                        "reverseN",
                        "overlap",
                        "swapLayering",
                    ],
                },
            ],
        },
        // The selected Fill sub-tab is the fill type.
        TabStrip {
            key: "fillTab",
            tabs: &[
                Tab {
                    caption: "Solid",
                    keys: &["solidColor", "customColor"],
                },
                Tab {
                    caption: "Gradient",
                    keys: &["gradStyle", "gradFrom", "gradTo", "srgbGradient"],
                },
                Tab {
                    caption: "Metallic",
                    keys: &["metal", "depth", "metalStrength", "metalHard", "light"],
                },
                Tab {
                    caption: "Image",
                    keys: &[
                        "texture",
                        "textureAngle",
                        "scale",
                        "mirror",
                        "textureHue",
                        "textureSat",
                    ],
                },
            ],
        },
    ],
    // The family a missing saved font falls back from, and to, for this session only (R157).
    transient_keys: &["fallbackFont", "fallbackFamily"],
    controls: &[
        ("text", NO),
        ("font", caption("Font: ")),
        ("bold", NO),
        ("italics", NO),
        ("size", NO),
        ("offsetX", NO),
        ("offsetY", NO),
        ("maintain", NO),
        ("solidColor", caption("Color: ")),
        (
            "customColor",
            ControlUi {
                note: Note::Text(
                    "\nThe solid color will be used if Fill > Image > Clipboard is\nchosen and the clipboard does not contain an image.",
                ),
                ..NO
            },
        ),
        ("gradStyle", caption("Style: ")),
        ("gradFrom", NO_RESET),
        ("gradTo", NO_RESET),
        ("srgbGradient", NO),
        ("metal", caption("Metal: ")),
        ("depth", NO),
        ("metalStrength", STRENGTH),
        ("metalHard", NO),
        ("light", ANGLE),
        ("texture", caption("Texture: ")),
        ("textureAngle", ANGLE),
        (
            "scale",
            ControlUi {
                decimals: Some(3),
                ..NO
            },
        ),
        ("mirror", NO),
        (
            "textureHue",
            ControlUi {
                track: Some(&TRACK_HUE),
                ..NO
            },
        ),
        // Leaves Paint.NET's default header line; its empty label shows none.
        (
            "textureSat",
            ControlUi {
                track: Some(&TRACK_SATURATION),
                ..ControlUi::DEFAULT
            },
        ),
        ("outlineColor", NO),
        ("outlineSize", NO),
        ("corner", caption("Outline corner style: ")),
        (
            "outlineOnTop",
            ControlUi {
                header: Some(" "),
                ..NO
            },
        ),
        ("shadowColor", NO_RESET),
        ("shadowLength", NO),
        ("shadowStrength", STRENGTH),
        ("bevelStrength", STRENGTH),
        (
            "highlight",
            ControlUi {
                note: Note::Text(
                    "3D light direction is the same as on the Shadow tab.\n3D for Metal fills are configured on Fill > Metallic tab.",
                ),
                ..NO_RESET
            },
        ),
        ("hardBevel", NO),
        ("overlayStyle", NO),
        ("rotate", caption("Rotate: ")),
        ("rotateAngle", SWING),
        ("jumble", SWING),
        (
            "reverse",
            ControlUi {
                caption: "Яeverse: ",
                ..ControlUi::DEFAULT
            },
        ),
        ("reverseN", NO),
        ("overlap", NO),
        ("swapLayering", NO),
    ],
    enabled,
    defaults,
    fonts,
    font_fallback,
    // spec-question: Q13 Text Fun Factory plain help credits Welshblue
    help: Help::Text(&["Text Fun Factory v2.2"]),
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    let m = manifest("bbTextFunFactory").unwrap();
    vec![EffectDef {
        id: "bbTextFunFactory",
        icon: m.icon,
        category: m.category,
        controls: controls(),
        kind: EffectKind::Gpu(GPU_CONTEXT, render),
        dialog: None,
        dialog_title: Some("Text Fun Factory"),
        indirect: Some(&UI),
    }]
}

type Choices = &'static [(&'static str, &'static str)];

fn controls() -> Vec<Control> {
    let check = |key, label, default| Control::Checkbox {
        key,
        label,
        default,
    };
    let colour = |key, label, rgb| Control::Color {
        key,
        label,
        default: ColorDefault::Rgb(rgb),
    };
    let select = |key, label, default, choices: Choices| Control::Select {
        key,
        label,
        default,
        choices,
    };
    let strength = |key, default| {
        range(key, "Strength", 0.0, 1.0, default)
            .step(0.01)
            .slider_keys(3, 0.05, 0.25, false)
    };
    // R14 replacement slider: explicit port key policy; R107 numeric AngleChooser increment is 1.
    let angle = |key, label, default| {
        range(key, label, -180.0, 180.0, default).slider_keys(3, 1.0, 5.0, false)
    };
    vec![
        Control::Value {
            key: "tab",
            default: || json!(0),
        },
        Control::Value {
            key: "fillTab",
            default: || json!(0),
        },
        Control::Value {
            key: "fallbackFont",
            default: || json!(""),
        },
        Control::Value {
            key: "fallbackFamily",
            default: || json!(""),
        },
        Control::Text {
            key: "text",
            label: "",
            default: "Enter Text Here",
            max_len: 1024,
            multiline: false,
        },
        // R188: BB30's drop-down; the dynamic default (Q9) replaces this static one.
        Control::FontSelect {
            key: "font",
            label: "",
            default: SANS_FONT,
        },
        check("bold", "Bold", false),
        check("italics", "Italics", false),
        range("size", "Size", 10.0, 500.0, 114.0),
        range(
            "offsetX",
            "\nFine tune text placement (Left-Right / Up-Down)",
            -50.0,
            50.0,
            10.0,
        ),
        range("offsetY", "", -50.0, 50.0, 0.0),
        check("maintain", "Maintain background", false),
        select(
            "solidColor",
            " ",
            "red",
            &[
                ("red", "Default (Red)"),
                ("primary", "Primary Color"),
                ("secondary", "Secondary Color"),
                ("custom", "Custom"),
                ("currentPalette", "From Current Palette"),
                ("defaultPalette", "From Default Palette"),
            ],
        ),
        colour("customColor", "\nCustom Color", 0xFF0000),
        select(
            "gradStyle",
            "",
            "vertical",
            &[
                ("vertical", "Vertical Gradient"),
                ("horizontal", "Horizontal Gradient"),
                ("topLeft", "Top Left Gradient"),
                ("topRight", "Top Right Gradient"),
            ],
        ),
        colour("gradFrom", "From", 0xFF0000),
        colour("gradTo", "To", 0x0000FF),
        check("srgbGradient", "sRGB gradient", true),
        select(
            "metal",
            "",
            "silver",
            &[
                ("silver", "Silver"),
                ("copper", "Copper"),
                ("gold", "Gold"),
                ("chrome", "Chrome"),
            ],
        ),
        range("depth", "3D Depth", 0.0, 30.0, 7.0),
        strength("metalStrength", 1.0),
        check("metalHard", "Hard bevel", true),
        angle("light", "Light direction", -45.0),
        select("texture", "", "t0", TEXTURE_CHOICES.as_slice()),
        angle("textureAngle", "", 90.0),
        range("scale", "Scale image", 0.06, 16.0, 1.0)
            .step(0.01)
            .slider_keys(3, 0.05, 0.25, true)
            .exponential(),
        check("mirror", "Mirror image when repeating", false),
        range("textureHue", "", -180.0, 180.0, 0.0),
        range("textureSat", "", 0.0, 200.0, 100.0),
        colour("outlineColor", "\nColor", 0),
        range("outlineSize", "Size", 0.0, 10.0, 3.0),
        select(
            "corner",
            "",
            "round",
            &[
                ("default", "Default"),
                ("round", "Round"),
                ("miter", "Miter"),
                ("bevel", "Bevel"),
                ("miterOrBevel", "Miter or Bevel"),
            ],
        ),
        check("outlineOnTop", "Outline on top of fill", false),
        colour("shadowColor", "Color", 0),
        range("shadowLength", "Length", 0.0, 50.0, 5.0),
        strength("shadowStrength", 0.75),
        strength("bevelStrength", 0.75),
        colour("highlight", "Highlight", 0xFFFFFF),
        check("hardBevel", "Hard bevel", false),
        check("overlayStyle", "Overlay style", false),
        select(
            "rotate",
            "",
            "none",
            &[
                ("none", "no letters"),
                ("same", "every letter the same direction"),
                ("everyOther", "every other letter"),
                ("backForth", "back and forth"),
                ("backForthTight", "back and forth (tight)"),
                ("firstOfWord", "only first letter of each word"),
            ],
        ),
        range("rotateAngle", "", -90.0, 90.0, 0.0),
        range("jumble", JUMBLE.as_str(), -50.0, 50.0, 0.0),
        select(
            "reverse",
            "",
            "none",
            &[
                ("none", "no letters"),
                ("every", "every number of letters:"),
                ("only", "only letter number:"),
                ("firstOfWord", "the first letter of each word"),
            ],
        ),
        range("reverseN", "", 1.0, 100.0, 2.0),
        range("overlap", OVERLAP.as_str(), -100.0, 50.0, -12.0),
        check("swapLayering", "Swap layering direction", false),
    ]
}

/// The Image sub-tab's texture: 0…48 built in, the last one the clipboard image (BB33).
fn texture_index(v: &Values) -> usize {
    choice(v, "texture")
        .strip_prefix('t')
        .and_then(|n| n.parse::<usize>().ok())
        .filter(|&i| i < TEXTURE_NAMES.len())
        .unwrap_or(0)
}

/// The selected Fill sub-tab: 0 Solid, 1 Gradient, 2 Metal, 3 Image.
fn fill_tab(v: &Values) -> usize {
    num(v, "fillTab").clamp(0.0, 3.0) as usize
}

/// The selected Fill sub-tab is Image on "Use Image on Clipboard" (the outer tab does not count).
fn clipboard_fill(v: &Values) -> bool {
    fill_tab(v) == 3 && texture_index(v) + 1 == TEXTURE_NAMES.len()
}

/// "Disabled when" / "Enabled only when" (Controls). Disabled values are kept.
fn enabled(v: &Values, key: &str) -> bool {
    match key {
        "customColor" => choice(v, "solidColor") == "custom",
        "outlineColor" | "corner" | "outlineOnTop" => num(v, "outlineSize") > 0.0,
        "shadowColor" | "shadowStrength" => num(v, "shadowLength") > 0.0,
        "bevelStrength" | "highlight" | "hardBevel" | "overlayStyle" => num(v, "depth") > 0.0,
        "metalStrength" | "metalHard" => !matches!(choice(v, "metal"), "gold" | "chrome"),
        "rotateAngle" => choice(v, "rotate") != "none",
        "reverseN" => matches!(choice(v, "reverse"), "every" | "only"),
        _ => true,
    }
}

/// R188: Q9 searches the document font list the Font drop-down shows.
fn defaults(v: &mut Values, _: &EffectEnv<'_>) {
    let choices = crate::fonts::document_choices();
    let choices = choices.iter().map(|(f, n)| (f.as_str(), n.as_str()));
    v.insert("font".into(), json!(default_font(choices)));
}

// spec-question: Q9 Text Fun Factory's default font
/// The first family, in display-name order (case-insensitive), whose name contains "Rounded"
/// (case-insensitive); else `SANS_FONT`.
pub fn default_font<'a>(choices: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut choices: Vec<_> = choices.into_iter().collect();
    choices.sort_by_cached_key(|(_, name)| name.to_lowercase());
    choices
        .into_iter()
        .find(|(_, name)| name.to_lowercase().contains("rounded"))
        .map_or_else(|| SANS_FONT.to_string(), |(family, _)| family.to_string())
}

/// The family this session draws: the chosen one, or the default `font_fallback` picked once
/// it failed to load ("A font name saved in the settings that is no longer installed": the
/// default font).
fn family(v: &Values) -> &str {
    let font = super::text(v, "font");
    if !font.is_empty() && font != super::text(v, "fallbackFont") {
        return font;
    }
    match super::text(v, "fallbackFamily") {
        "" => SANS_FONT,
        family => family,
    }
}

fn style(v: &Values) -> FaceStyle {
    TEXT_FUN_BOLD_FACE_FALLBACK(flag(v, "bold"), flag(v, "italics"))
}

/// R157: the face the letters and their prefixes are drawn with, and the lazy colour face a
/// colour-emoji prefix is measured with.
fn fonts(v: &Values) -> Vec<(String, FaceStyle)> {
    let face = family(v);
    let mut out = vec![(face.to_string(), style(v))];
    let s = settings(face, flag(v, "bold"), flag(v, "italics"));
    if needs_color_font(super::text(v, "text"), &s) {
        out.push(("Noto Color Emoji".into(), FaceStyle::Regular));
    }
    out
}

/// R157 / Q69: a chosen family that failed to load falls back, for the session, to the Q9
/// default, and that one failing too to `SANS_FONT`; the saved token keeps its name. Another
/// face's failure is not a reason to drop a ready family. The default is picked here, on the UI
/// thread: a render worker's `document_choices` is the bundled list only.
fn font_fallback(v: &mut Values) -> bool {
    let font = super::text(v, "font").to_string();
    let face = family(v).to_string();
    if face == SANS_FONT || crate::fonts::registered_face(&face, style(v)).is_some() {
        return false;
    }
    let mut to = SANS_FONT.to_string();
    if face == font {
        let choices = crate::fonts::document_choices();
        to = default_font(choices.iter().map(|(f, n)| (f.as_str(), n.as_str())));
    }
    if to == face {
        to = SANS_FONT.into();
    }
    v.insert("fallbackFont".into(), json!(font));
    v.insert("fallbackFamily".into(), json!(to));
    true
}

/// The letter counters (Behaviour "Letter counters"), for one non-space letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LetterCounters {
    /// The 1st, 3rd, 5th … non-space letter.
    pub odd: bool,
    /// 1, 2, 3, 0, 1, … from the first letter.
    pub phase: u8,
    /// The first non-space letter of the text or after a space.
    pub word_start: bool,
    /// The number of non-space letters before this one.
    pub index: usize,
}

/// One counter set per UTF-16 unit; `None` for U+0020, the only space.
pub fn counters(text: &str) -> Vec<Option<LetterCounters>> {
    let (mut index, mut after_space) = (0, true);
    text.encode_utf16()
        .map(|u| {
            if u == 0x20 {
                after_space = true;
                return None;
            }
            let c = LetterCounters {
                odd: index % 2 == 0,
                phase: ((index + 1) % 4) as u8,
                word_start: after_space,
                index,
            };
            (index, after_space) = (index + 1, false);
            Some(c)
        })
        .collect()
}

/// The Rotate rule's angle for a letter, degrees clockwise.
pub fn letter_angle(rule: &str, c: &LetterCounters, a: f64) -> f64 {
    match rule {
        "same" => a,
        "everyOther" if c.odd => a,
        "backForth" => match c.phase {
            2 => -a,
            0 => a,
            _ => 0.0,
        },
        "backForthTight" if c.odd => a,
        "backForthTight" => -a,
        "firstOfWord" if c.word_start => a,
        _ => 0.0,
    }
}

/// Whether the Reverse rule (with its number `n`) mirrors a letter.
pub fn letter_mirrored(rule: &str, n: u32, c: &LetterCounters) -> bool {
    let nth = c.index as u64 + 1;
    match rule {
        "every" => n > 0 && nth.is_multiple_of(n as u64),
        "only" => nth == n as u64,
        "firstOfWord" => c.word_start,
        _ => false,
    }
}

/// How far Jumble `j` moves a letter down: odd letters for j > 0, even ones for j < 0.
pub fn letter_drop(j: f64, c: &LetterCounters) -> f64 {
    if (j > 0.0 && c.odd) || (j < 0.0 && !c.odd) {
        j.abs()
    } else {
        0.0
    }
}

/// `TEXT_FUN_RAW_UTF16_UNITS`.
pub fn raw_units(units: &[u16], family: &str, px: f32, bold: bool, italic: bool) -> RawUnits {
    let drawn: Vec<Vec<u16>> = units
        .iter()
        .filter(|&&u| u != 0x20)
        .map(|&u| vec![u, 0xfe0e])
        .collect();
    let (widths, lines) = layout_raw_units(units, &drawn, family, px, bold, italic);
    let mut lines = lines.into_iter();
    let glyphs = units
        .iter()
        .map(|&u| if u == 0x20 { None } else { lines.next() })
        .collect();
    (widths, glyphs)
}

/// A letter: its counters (`None` for a space), its layout box's top-left and its final outline
/// items (none without ink).
pub struct Letter {
    pub counters: Option<LetterCounters>,
    pub origin: [f64; 2],
    pub paths: Vec<Path>,
}

/// Every unit's letter, laid out from the selection bounds' top-left (Behaviour "Per-letter
/// layout", "Letter transforms").
pub fn letters(v: &Values, env: &EffectEnv) -> Vec<Letter> {
    let text = super::text(v, "text");
    let units: Vec<u16> = text.encode_utf16().collect();
    let (face, bold, italic) = (family(v), flag(v, "bold"), flag(v, "italics"));
    // Font em size = Size in pixels.
    let px = num(v, "size").clamp(10.0, 500.0) as f32;
    let (widths, glyphs) = TEXT_FUN_RAW_UTF16_UNITS(&units, face, px, bold, italic);
    let missing = std::cell::OnceCell::new();
    let x0 = env.bounds.x as f64 + num(v, "offsetX").clamp(-50.0, 50.0);
    let y0 = env.bounds.y as f64 + num(v, "offsetY").clamp(-50.0, 50.0);
    let overlap = num(v, "overlap").clamp(-100.0, 50.0);
    counters(text)
        .into_iter()
        .zip(widths.iter().zip(&glyphs))
        .enumerate()
        .map(|(k, (c, (prefix, glyph)))| {
            let origin = [x0 + prefix + k as f64 * overlap, y0];
            let paths = c.zip(glyph.as_ref()).map_or_else(Vec::new, |(c, glyph)| {
                // R158 Q68: the projection leaves no colour bitmap; should one appear it draws
                // the visible missing-glyph outline (U+FFFF is a noncharacter) instead.
                let glyph = if glyph.items.iter().any(|i| matches!(i, DrawItem::Bitmap(_))) {
                    missing.get_or_init(|| {
                        layout_raw_units(&[], &[vec![0xffff]], face, px, bold, italic)
                            .1
                            .remove(0)
                    })
                } else {
                    glyph
                };
                turn(outlines(glyph, origin), &c, v)
            });
            Letter {
                counters: c,
                origin,
                paths,
            }
        })
        .collect()
}

/// A unit line's outline items with its box's top-left at `origin`.
fn outlines(line: &TextLine, origin: [f64; 2]) -> Vec<Path> {
    let at = Transform::from_translate(origin[0] as f32, origin[1] as f32);
    line.items
        .iter()
        .filter_map(|i| match i {
            DrawItem::Outline(p) => p.clone().transform(at),
            DrawItem::Bitmap(_) => None,
        })
        .collect()
}

/// The letter transforms in order: mirror about the box's vertical centre line, turn clockwise
/// about the box centre, then jumble down.
fn turn(paths: Vec<Path>, c: &LetterCounters, v: &Values) -> Vec<Path> {
    let Some(b) = ink(&paths) else {
        return paths;
    };
    let (cx, cy) = ((b.left() + b.right()) / 2.0, (b.top() + b.bottom()) / 2.0);
    let mut t = Transform::identity();
    let n = num(v, "reverseN").clamp(1.0, 100.0) as u32;
    if letter_mirrored(choice(v, "reverse"), n, c) {
        t = t.post_concat(Transform::from_row(-1.0, 0.0, 0.0, 1.0, 2.0 * cx, 0.0));
    }
    let a = letter_angle(
        choice(v, "rotate"),
        c,
        num(v, "rotateAngle").clamp(-90.0, 90.0),
    );
    if a != 0.0 {
        t = t.post_concat(rotation(a, TEXT_FUN_ROTATION_CLOCKWISE, [cx, cy]));
    }
    let drop = letter_drop(num(v, "jumble").clamp(-50.0, 50.0), c) as f32;
    t = t.post_concat(Transform::from_translate(0.0, drop));
    paths.into_iter().filter_map(|p| p.transform(t)).collect()
}

/// A turn by `a` degrees about `centre`: clockwise on screen (y down) for positive `a`, or the
/// other way when not `clockwise` (`TEXT_FUN_ROTATION_CLOCKWISE`).
pub fn rotation(a: f64, clockwise: bool, [cx, cy]: [f32; 2]) -> Transform {
    let a = if clockwise { a } else { -a };
    let (s, k) = a.to_radians().sin_cos();
    let (s, k) = (s as f32, k as f32);
    Transform::from_row(k, s, -s, k, cx - k * cx + s * cy, cy - s * cx - k * cy)
}

/// The union of the paths' tight bounds; `None` without ink.
pub fn ink(paths: &[Path]) -> Option<Rect> {
    paths
        .iter()
        .filter_map(Path::compute_tight_bounds)
        .reduce(union)
}

fn union(a: Rect, b: Rect) -> Rect {
    Rect::from_ltrb(
        a.left().min(b.left()),
        a.top().min(b.top()),
        a.right().max(b.right()),
        a.bottom().max(b.bottom()),
    )
    .unwrap_or(a)
}

/// The outline pen: `corner` as the line join ("Default" is Direct2D's default stroke style,
/// miter limit 10), flat caps.
pub fn outline_pen(corner: &str, width: f32) -> Pen {
    Pen {
        width,
        join: match corner {
            "round" => Join::Round,
            "bevel" => Join::Bevel,
            "miterOrBevel" => Join::MiterOrBevel,
            _ => Join::Miter,
        },
        cap: Cap::Flat,
        miter_limit: 10.0,
        dash: None,
    }
}

/// `TEXT_FUN_GRADIENT_RAW_WORKING`.
pub fn gradient_raw_working(u0: [u8; 3], u1: [u8; 3], t: f32, srgb: bool) -> [f32; 3] {
    let raw = |b: u8| b as f32 / 255.0;
    if t <= 0.0 {
        return u0.map(raw);
    }
    if t >= 1.0 {
        return u1.map(raw);
    }
    std::array::from_fn(|i| {
        let (a, b) = (raw(u0[i]), raw(u1[i]));
        if srgb {
            a + (b - a) * t
        } else {
            let (a, b) = (srgb_to_linear_f32(a), srgb_to_linear_f32(b));
            linear_to_srgb_f32(a + (b - a) * t)
        }
    })
}

/// A linear gradient's parameter at `p` from `s` to `e`: the perpendicular projection, clamped
/// to 0…1; 0 (From) on a zero-length axis.
pub fn gradient_t(p: [f32; 2], s: [f32; 2], e: [f32; 2]) -> f32 {
    let d = [e[0] - s[0], e[1] - s[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    if len2 <= 0.0 || len2.is_nan() {
        return 0.0;
    }
    (((p[0] - s[0]) * d[0] + (p[1] - s[1]) * d[1]) / len2).clamp(0.0, 1.0)
}

/// The Image fill's Hue / Saturation on straight working RGB: saturation factor = slider / 100.
/// The shared helper triples its percent's excess over 100, so the slider maps to p = s up to
/// 100 and 100 + (s − 100)/3 above (R158); hue in degrees, lightness unchanged.
pub fn hue_saturation(rgb: [f32; 3], hue: f64, sat: f64) -> [f32; 3] {
    let p = if sat <= 100.0 {
        sat
    } else {
        100.0 + (sat - 100.0) / 3.0
    };
    hsl_adjust(rgb, hue as f32, p as f32, 0.0)
}

/// (cos, sin) of `deg`, exact at the quarter turns.
fn cos_sin_deg(deg: f64) -> (f64, f64) {
    let d = deg.rem_euclid(360.0);
    for (q, cs) in [
        (0.0, (1.0, 0.0)),
        (90.0, (0.0, 1.0)),
        (180.0, (-1.0, 0.0)),
        (270.0, (0.0, -1.0)),
    ] {
        if d == q {
            return cs;
        }
    }
    let r = d.to_radians();
    (r.cos(), r.sin())
}

/// R158: a tiled image brush tied to the canvas origin: the source in the working space with
/// Hue / Saturation applied once, premultiplied, scaled then turned clockwise by 90° − angle,
/// repeated (Wrap) or mirrored, sampled with cubic taps. BB33 passes the clipboard image.
pub struct Brush {
    src: EffectSurface,
    edge: Edge,
    inverse: [f64; 4],
}

impl Brush {
    pub fn new(
        img: &ImageData,
        hue: f64,
        sat: f64,
        angle: f64,
        scale: f64,
        mirror: bool,
        info: RenderInfo,
    ) -> Self {
        let mut src = EffectSurface::from_rgba8(img, info.space, Alpha::Straight);
        let identity = hue == 0.0 && sat == 100.0;
        for p in &mut src.px {
            if !identity {
                let [r, g, b] = hue_saturation([p[0], p[1], p[2]], hue, sat);
                *p = [r, g, b, p[3]];
            }
            *p = premultiply(*p);
        }
        let (c, s) = cos_sin_deg(90.0 - angle);
        Self {
            src,
            edge: if mirror { Edge::Mirror } else { Edge::Wrap },
            inverse: [c / scale, s / scale, -s / scale, c / scale],
        }
    }

    /// The premultiplied brush at canvas pixel (x, y)'s centre.
    // approximation: D2D high-quality cubic (text-formations-text-fun-factory.md "Image")
    pub fn at(&self, x: i32, y: i32) -> [f32; 4] {
        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
        let [a, b, c, d] = self.inverse;
        let s = [(a * px + b * py) as f32, (c * px + d * py) as f32];
        filter4(s, catmull_rom, |i, j| self.src.fetch(i, j, self.edge))
    }
}

/// The letters' fill, decided by the selected Fill sub-tab; premultiplied working colours.
enum Fill {
    Flat([f32; 4]),
    /// The palette entry at each letter's running index modulo 96.
    Palette(Vec<[f32; 4]>),
    /// From/To stop bytes, "sRGB gradient", the axis over a letter's box.
    Gradient([u8; 3], [u8; 3], bool, fn(Rect) -> ([f32; 2], [f32; 2])),
    Image(Brush),
}

impl Fill {
    fn new(v: &Values, env: &EffectEnv, info: RenderInfo) -> Self {
        let rgb = |key| {
            let [r, g, b, _] = color_value(v, key, env);
            [r, g, b]
        };
        match fill_tab(v) {
            1 => Fill::Gradient(
                rgb("gradFrom"),
                rgb("gradTo"),
                flag(v, "srgbGradient"),
                match choice(v, "gradStyle") {
                    "horizontal" => |b| ([b.left(), b.top()], [b.right(), b.top()]),
                    "topLeft" => |b| ([b.left(), b.top()], [b.right(), b.bottom()]),
                    "topRight" => |b| ([b.right(), b.top()], [b.left(), b.bottom()]),
                    _ => |b| ([b.left(), b.top()], [b.left(), b.bottom()]),
                },
            ),
            // Gold and Chrome replace the fill altogether (`metal_face`).
            2 => Fill::Flat(match choice(v, "metal") {
                "copper" => colour_exact([165, 25, 0, 255], info),
                _ => colour_exact([128, 128, 128, 255], info),
            }),
            3 => {
                let built_in;
                let img = if clipboard_fill(v) {
                    // The clipboard image at its own size (BB33); without one (a zero-sized one
                    // is none), the Solid fill.
                    match env.image.filter(|i| i.width > 0 && i.height > 0) {
                        Some(img) => img,
                        None => return Self::solid(v, env, info),
                    }
                } else {
                    built_in = texture(texture_index(v));
                    &built_in
                };
                Fill::Image(Brush::new(
                    img,
                    num(v, "textureHue").clamp(-180.0, 180.0),
                    num(v, "textureSat").clamp(0.0, 200.0),
                    num(v, "textureAngle").clamp(-180.0, 180.0),
                    num(v, "scale").clamp(0.06, 16.0),
                    flag(v, "mirror"),
                    info,
                ))
            }
            _ => Self::solid(v, env, info),
        }
    }

    /// The Solid sub-tab's colour, converted exactly, alpha forced opaque.
    fn solid(v: &Values, env: &EffectEnv, info: RenderInfo) -> Self {
        let exact = |c: Px| colour_exact([c[0], c[1], c[2], 255], info);
        // `DEFAULT_PALETTE` is AARRGGBB.
        let default = |i: usize| {
            let [_, r, g, b] = crate::render::color_ui::DEFAULT_PALETTE[i].to_be_bytes();
            [r, g, b, 255]
        };
        match choice(v, "solidColor") {
            "primary" => Fill::Flat(exact(env.primary)),
            "secondary" => Fill::Flat(exact(env.secondary)),
            "custom" => Fill::Flat(exact(color_value(v, "customColor", env))),
            "currentPalette" => Fill::Palette(
                (0..96)
                    .map(|i| exact(env.palette.get(i).copied().unwrap_or_else(|| default(i))))
                    .collect(),
            ),
            "defaultPalette" => Fill::Palette((0..96).map(|i| exact(default(i))).collect()),
            _ => Fill::Flat(exact([255, 0, 0, 255])),
        }
    }

    /// The fill at canvas pixel (x, y) of letter `index` whose final box is `b`.
    fn at(&self, x: i32, y: i32, index: usize, b: Rect) -> [f32; 4] {
        match self {
            Fill::Flat(c) => *c,
            Fill::Palette(p) => p[index % 96],
            Fill::Gradient(from, to, srgb, axis) => {
                let (s, e) = axis(b);
                let t = gradient_t([x as f32 + 0.5, y as f32 + 0.5], s, e);
                let [r, g, b] = TEXT_FUN_GRADIENT_RAW_WORKING(*from, *to, t, *srgb);
                [r, g, b, 1.0]
            }
            Fill::Image(brush) => brush.at(x, y),
        }
    }
}

/// "3D bevel" step 5: the lighting map is clear within this distance of 0.5.
const DEAD_BAND: f32 = 1e-7;

/// The 3D bevel's settings: the 3D tab's, or Silver's and Copper's overrides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bevel {
    pub strength: f32,
    /// Opaque, raw.
    pub highlight: [f32; 4],
    pub hard: bool,
    pub overlay: bool,
}

/// What a letter's fill becomes (Behaviour "Fill" Metallic, "3D bevel", "Gold and Chrome").
#[derive(Debug, PartialEq)]
enum Face {
    /// As it is (3D Depth 0).
    Flat,
    Bevel(Bevel),
    /// Gold (true) or Chrome's own pipeline, in place of the fill.
    Metal(bool),
}

impl Face {
    fn new(v: &Values, env: &EffectEnv, depth: f64) -> Self {
        let metal = (fill_tab(v) == 2).then(|| choice(v, "metal"));
        if let Some(m @ ("gold" | "chrome")) = metal {
            return Face::Metal(m == "gold");
        }
        if depth <= 0.0 {
            return Face::Flat;
        }
        let strength = |key| num(v, key).clamp(0.0, 1.0) as f32;
        let overlay = flag(v, "overlayStyle");
        // quirk(bb): text-formations-text-fun-factory.md "Colour handling" (highlights raw)
        Face::Bevel(match metal {
            Some(m) => Bevel {
                strength: strength("metalStrength"),
                highlight: colour_raw(if m == "copper" {
                    [163, 72, 58, 255]
                } else {
                    [200, 200, 200, 255]
                }),
                hard: flag(v, "metalHard"),
                overlay,
            },
            None => {
                let [r, g, b, _] = color_value(v, "highlight", env);
                Bevel {
                    strength: strength("bevelStrength"),
                    highlight: colour_raw([r, g, b, 255]),
                    hard: flag(v, "hardBevel"),
                    overlay,
                }
            }
        })
    }

    /// R159: the reach its filters add to a letter's buffer, blur support plus the Emboss
    /// neighbour.
    fn reach(&self, depth: f64) -> i32 {
        let bokeh = (bokeh_len(depth) / 2) as i32 + 1;
        match self {
            Face::Flat => 0,
            Face::Bevel(Bevel { hard: false, .. }) => {
                (gaussian_kernel(depth * RADIUS_TO_SIGMA).len() / 2) as i32 + 1
            }
            Face::Bevel(_) | Face::Metal(_) => bokeh,
        }
    }
}

/// "3D bevel" step 1: the opaque height mask over a letter's buffer `w` wide, white with the
/// letter black (1 − its coverage), each value through `f`.
fn height(cover: &[f32], w: u32, f: impl Fn(f32) -> f32 + Sync) -> EffectSurface {
    EffectSurface::from_fn(w, cover.len() as u32 / w, |x, y| {
        let v = f(1.0 - cover[(y * w + x) as usize]);
        [v, v, v, 1.0]
    })
}

/// Step 3's encode, on straight RGB only: the standard sRGB curve whatever the working space
/// (a numeric stage of the plugin's graph, not the output conversion).
fn encode(mut s: EffectSurface) -> EffectSurface {
    for p in &mut s.px {
        for c in &mut p[..3] {
            *c = linear_to_srgb_f32(*c);
        }
    }
    s
}

/// "3D bevel" steps 1–3 up to the shared quantise / Emboss tail: the height softened by depth
/// D, a Gaussian (σ = D/3, normalised once) or, `hard`, plain Bokeh quality 4; straight, encoded.
fn bevel_height(cover: &[f32], w: u32, depth: f64, hard: bool) -> EffectSurface {
    let h = height(cover, w, |v| v);
    encode(if hard {
        // approximation: Bokeh defaults as gamma boost 0, Mirror edge (text-formations-text-fun-factory.md "3D bevel" step 2)
        // spec-question: Q72 Text Bokeh defaults
        bokeh_with_edge(
            &h,
            depth,
            4,
            TEXT_FUN_BOKEH_GAMMA_BOOST,
            TEXT_FUN_BOKEH_EDGE,
        )
    } else {
        // spec-question: Q73 Text Gaussian high-quality interpretation
        const { assert!(TEXT_FUN_GAUSSIAN_SHARED_HQ) };
        // The opaque height is its own premultiplied form; the blurred one is not.
        let mut s = blur_soft(&h, depth * RADIUS_TO_SIGMA);
        for p in &mut s.px {
            *p = unpremultiply(*p);
        }
        s
    })
}

/// "Gold and Chrome" steps 1–3 up to the shared tail: the height's RGB raised to
/// `TEXT_FUN_METAL_MASK_POWER` (no inverse), Bokeh quality 10 (none at depth 0), encoded.
fn metal_height(cover: &[f32], w: u32, depth: f64) -> EffectSurface {
    // approximation: height power 2.2, not 1/2.2 (text-formations-text-fun-factory.md "Gold and Chrome" step 1)
    // spec-question: Q71 Text metal height power
    let h = height(cover, w, |v| v.powf(TEXT_FUN_METAL_MASK_POWER));
    // approximation: Bokeh defaults as gamma boost 0, Mirror edge (text-formations-text-fun-factory.md "Gold and Chrome" step 2)
    // spec-question: Q72 Text Bokeh defaults
    encode(bokeh_with_edge(
        &h,
        depth,
        10,
        TEXT_FUN_BOKEH_GAMMA_BOOST,
        TEXT_FUN_BOKEH_EDGE,
    ))
}

/// "3D bevel" steps 5–6 on one premultiplied fill pixel over relief `r`: the lighting map
/// (`highlight` above 0.5, black below, opacity Strength·|r − 0.5|/0.5 capped at 0.99, nothing
/// within `DEAD_BAND`) painted on the straight fill by source-over or, `overlay`, Overlay; the
/// result keeps its colour with the fill's own alpha, premultiplied again.
fn light_fill(
    fill: [f32; 4],
    r: f32,
    strength: f32,
    highlight: [f32; 4],
    overlay: bool,
) -> [f32; 4] {
    let Some((shade, o)) = lighting(r, strength, DEAD_BAND) else {
        return fill;
    };
    let [cr, cg, cb, _] = match shade {
        Shade::Light => highlight,
        Shade::Dark => [0.0; 4],
    };
    let layer = [cr, cg, cb, o];
    if overlay {
        with_alpha(mix(d2d_blend(11), fill, premultiply(layer)), fill[3])
    } else {
        let [r, g, b, _] = straight_over(unpremultiply(fill), layer);
        premultiply([r, g, b, fill[3]])
    }
}

/// "Gold and Chrome" step 5 on relief E: Gold, the Color Burn of the raw gold onto E; Chrome,
/// the Difference of E and 1 − E. Opaque straight RGB.
fn metal_colour(e: f32, gold: bool) -> [f32; 3] {
    // spec-question: Q74 Text metal relief range
    let e = if TEXT_FUN_METAL_CLAMP_RELIEF {
        e.clamp(0.0, 1.0)
    } else {
        e
    };
    let dst = [e, e, e, 1.0];
    let [r, g, b, _] = if gold {
        // quirk(bb): text-formations-text-fun-factory.md "Colour handling" (the gold raw)
        mix(d2d_blend(5), dst, colour_raw([255, 215, 0, 255]))
    } else {
        mix(d2d_blend(18), dst, [1.0 - e, 1.0 - e, 1.0 - e, 1.0])
    };
    [r, g, b]
}

/// "3D bevel" on one letter: its premultiplied covered `fill` lit by the relief of its own
/// coverage `cover` (the same buffer) at "3D Depth" `depth` (0: the fill as is) and "Light
/// direction" `light`.
pub fn bevel_letter(
    fill: &EffectSurface,
    cover: &[f32],
    depth: f64,
    light: f64,
    b: &Bevel,
) -> EffectSurface {
    if depth <= 0.0 {
        return fill.clone();
    }
    let relief = relief_from_soft(&bevel_height(cover, fill.w, depth, b.hard), light);
    EffectSurface::from_fn(fill.w, fill.h, |x, y| {
        light_fill(
            fill.at(x, y),
            relief.at(x, y)[0],
            b.strength,
            b.highlight,
            b.overlay,
        )
    })
}

/// "Gold and Chrome" on one letter (`gold`, else Chrome) from its coverage `cover` over a
/// buffer `w` wide: the metal colour of its relief, the coverage as alpha (once), premultiplied.
pub fn metal_face(cover: &[f32], w: u32, depth: f64, light: f64, gold: bool) -> EffectSurface {
    let relief = relief_from_soft(&metal_height(cover, w, depth), light);
    EffectSurface::from_fn(w, relief.h, |x, y| {
        let [r, g, b] = metal_colour(relief.at(x, y)[0], gold);
        premultiply([r, g, b, cover[(y * w + x) as usize]])
    })
}

/// `b` widened by `reach` px and rounded out, within `limit`.
fn around(b: Rect, reach: i32, limit: IntRect) -> IntRect {
    let clamp = |v: f32, lo: i32, hi: i32| (v as f64).clamp(lo as f64, hi as f64) as i32;
    let r = reach as f32;
    let (x0, x1) = (
        clamp(b.left().floor() - r, limit.x, limit.right()),
        clamp(b.right().ceil() + r, limit.x, limit.right()),
    );
    let (y0, y1) = (
        clamp(b.top().floor() - r, limit.y, limit.bottom()),
        clamp(b.bottom().ceil() + r, limit.y, limit.bottom()),
    );
    IntRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
}

fn render(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let info = env.render_info.unwrap_or(GPU_CONTEXT);
    let (w, h) = (src.w, src.h);
    let mut stack = EffectSurface::new(w, h);
    let letters = letters(v, env);
    if letters.iter().any(|l| !l.paths.is_empty()) {
        let fill = Fill::new(v, env, info);
        let opaque = |key| {
            let [r, g, b, _] = color_value(v, key, env);
            [r, g, b, 255]
        };
        // quirk(bb): text-formations-text-fun-factory.md "Shadow" (always applied; opacity
        // rounded down to a level) and "Colour handling" (outline and shadow colours raw)
        let len = num(v, "shadowLength").clamp(0.0, 50.0);
        let sigma = len * RADIUS_TO_SIGMA;
        let light = num(v, "light").clamp(-180.0, 180.0);
        let depth = num(v, "depth").clamp(0.0, 30.0);
        let face = Face::new(v, env, depth);
        let offset = shadow_offset(len, light + SHADOW_ANGLE_OFFSET_DEG);
        let strength = (num(v, "shadowStrength").clamp(0.0, 1.0) * 255.0).floor() as f32 / 255.0;
        let shadow = colour_raw(opaque("shadowColor")).map(|c| c * strength);
        let rim = colour_raw(opaque("outlineColor"));
        let size = num(v, "outlineSize").clamp(0.0, 10.0) as f32;
        let pen = (size > 0.0).then(|| outline_pen(choice(v, "corner"), size));
        // R158 halo: the shadow's Gaussian support, its shift and the bilinear tap past it;
        // R159: plus the bevel's.
        let shift = offset[0].abs().max(offset[1].abs()).ceil() as i32;
        let reach = (gaussian_kernel(sigma).len() / 2) as i32 + shift + 2 + face.reach(depth);
        let canvas = IntRect::new(0, 0, w, h);
        let halo = IntRect::new(-reach, -reach, w + 2 * reach as u32, h + 2 * reach as u32);
        let (on_top, swap) = (flag(v, "outlineOnTop"), flag(v, "swapLayering"));
        for l in &letters {
            let (Some(c), Some(shape)) = (l.counters, ink(&l.paths)) else {
                continue;
            };
            let strokes: Vec<Path> = pen.map_or_else(Vec::new, |p| {
                l.paths.iter().filter_map(|q| widen(q, &p)).collect()
            });
            let drawn = ink(&strokes).map_or(shape, |s| union(s, shape));
            let r = around(drawn, reach, halo);
            if r.intersect(&canvas).is_empty() {
                continue;
            }
            // The letter's own buffer over r, in canvas coordinates: each item's Winding
            // coverage combined source-over (the pre-stroke shape the bevel reads), the fill
            // sampled at canvas pixels and bevelled (or the metal face), the centred outline
            // under or over it, then its shadow.
            let local = Transform::from_translate(-r.x as f32, -r.y as f32);
            let local = |ps: &[Path]| -> Vec<Path> {
                ps.iter()
                    .filter_map(|p| p.clone().transform(local))
                    .collect()
            };
            let cover = coverage(&local(&l.paths), None, r.w, r.h);
            let stroke = coverage(&local(&strokes), None, r.w, r.h);
            let filled = match &face {
                Face::Metal(gold) => metal_face(&cover, r.w, depth, light, *gold),
                _ => {
                    let f = EffectSurface::from_fn(r.w, r.h, |x, y| {
                        let i = (y * r.w + x) as usize;
                        if cover[i] > 0.0 {
                            let at = fill.at(r.x + x as i32, r.y + y as i32, c.index, shape);
                            at.map(|p| p * cover[i])
                        } else {
                            [0.0; 4]
                        }
                    });
                    match &face {
                        Face::Bevel(b) => bevel_letter(&f, &cover, depth, light, b),
                        _ => f,
                    }
                }
            };
            let letter = EffectSurface::from_fn(r.w, r.h, |x, y| {
                let i = (y * r.w + x) as usize;
                let f = filled.px[i];
                let o = rim.map(|p| p * stroke[i]);
                if on_top {
                    source_over(f, o)
                } else {
                    source_over(o, f)
                }
            });
            let chain = drop_shadow_with(&letter, sigma, offset, shadow, false);
            // Each new letter goes under everything so far, or on top when swapped.
            let vis = r.intersect(&canvas);
            for y in vis.y..vis.bottom() {
                for x in vis.x..vis.right() {
                    let new = chain.at((x - r.x) as u32, (y - r.y) as u32);
                    let p = &mut stack.px[y as usize * w as usize + x as usize];
                    *p = if swap {
                        source_over(*p, new)
                    } else {
                        source_over(new, *p)
                    };
                }
            }
        }
    }
    let maintain = flag(v, "maintain");
    // Source-over is premultiplied: a straight boundary converts around it.
    EffectSurface::from_fn(w, h, |x, y| {
        let s = stack.at(x, y);
        let out = if maintain {
            let b = src.at(x, y);
            let b = if info.input == Alpha::Straight {
                premultiply(b)
            } else {
                b
            };
            source_over(b, s)
        } else {
            s
        };
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
    use crate::effect::bb::test_util::*;
    use crate::effect::{EffectEnv, find};
    use crate::pixel::Px;
    use serde_json::{Value, json};

    /// A statically registered bundled face with U+2588 (a solid 600 × 1213-unit block) and F.
    const FONT: &str = "Noto Sans Mono";
    const W: u32 = 400;
    const H: u32 = 300;

    fn base(extra: Value) -> Value {
        let mut v = json!({"text": "█", "font": FONT, "size": 114, "outlineSize": 0, "depth": 0,
            "shadowLength": 0, "rotate": "none", "jumble": 0, "reverse": "none", "overlap": 0,
            "offsetX": 10, "offsetY": 0, "maintain": false});
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        v
    }
    fn white() -> ImageData {
        solid(W, H, [255; 4])
    }
    fn run(extra: Value) -> ImageData {
        bb("bbTextFunFactory", base(extra), &white())
    }
    fn run_env(extra: Value, env: &EffectEnv) -> ImageData {
        bb_env("bbTextFunFactory", base(extra), &white(), env)
    }
    fn run_alt(extra: Value) -> ImageData {
        bb_ctx(
            "bbTextFunFactory",
            super::super::conventions::ALT_GPU_CONTEXT,
            base(extra),
            &white(),
        )
    }
    fn values(extra: Value) -> Values {
        let def = find("bbTextFunFactory").unwrap();
        let env = EffectEnv::whole(W, H);
        let mut v = super::super::indirect_defaults(&def, &env);
        v.extend(base(extra).as_object().unwrap().clone());
        v
    }
    /// The letters' placed paths for these settings over the whole 400 × 300 layer.
    fn placed(extra: Value) -> Vec<Letter> {
        letters(&values(extra), &EffectEnv::whole(W, H))
    }
    /// A letter's final ink box [l, t, r, b].
    fn ink_box(l: &Letter) -> [f32; 4] {
        let b = ink(&l.paths).unwrap();
        [b.left(), b.top(), b.right(), b.bottom()]
    }
    fn bbox(img: &ImageData, f: impl Fn(Px) -> bool) -> (u32, u32, u32, u32) {
        let mut b = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..img.height {
            for x in 0..img.width {
                if f(px(img, x, y)) {
                    b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
                }
            }
        }
        b
    }
    fn palette_with(first: &[Px]) -> Vec<Px> {
        let mut pal = first.to_vec();
        pal.resize(96, [9, 9, 9, 255]);
        pal
    }
    fn palette_env(pal: &[Px]) -> EffectEnv<'_> {
        EffectEnv {
            palette: pal,
            ..EffectEnv::whole(W, H)
        }
    }

    /// One block at 114 px: 68.4 px wide from x = 10, its top 10.944 and bottom 149.226
    /// (Noto Sans Mono: ascent 1069, block −240…973 of 1000 units).
    #[test]
    fn block_geometry_is_the_fixture_face() {
        let b = ink_box(&placed(json!({}))[0]);
        let want = [10.0, 10.944, 78.4, 149.226];
        assert!(
            b.iter().zip(want).all(|(g, w)| (g - w).abs() < 0.01),
            "{b:?}"
        );
    }

    /// Letter counters: odd/even, phase of four, word starts, running index; only U+0020 is a
    /// space; each UTF-16 unit counts.
    #[test]
    fn counters_and_transforms() {
        let c = counters("ab cd");
        assert!(c[2].is_none());
        let l: Vec<_> = c.into_iter().flatten().collect();
        assert_eq!(
            l.iter()
                .map(|c| (c.odd, c.phase, c.word_start, c.index))
                .collect::<Vec<_>>(),
            [
                (true, 1, true, 0),
                (false, 2, false, 1),
                (true, 3, true, 2),
                (false, 0, false, 3)
            ]
        );
        assert_eq!(
            l.iter()
                .map(|c| letter_angle("backForth", c, 30.0))
                .collect::<Vec<_>>(),
            [0.0, -30.0, 0.0, 30.0]
        );
        let angles = |rule| {
            l.iter()
                .map(|c| letter_angle(rule, c, 30.0))
                .collect::<Vec<_>>()
        };
        assert_eq!(angles("none"), [0.0; 4]);
        assert_eq!(angles("same"), [30.0; 4]);
        assert_eq!(angles("everyOther"), [30.0, 0.0, 30.0, 0.0]);
        assert_eq!(angles("backForthTight"), [30.0, -30.0, 30.0, -30.0]);
        assert_eq!(angles("firstOfWord"), [30.0, 0.0, 30.0, 0.0]);
        assert_eq!(
            l.iter().map(|c| letter_drop(10.0, c)).collect::<Vec<_>>(),
            [10.0, 0.0, 10.0, 0.0]
        );
        assert_eq!(
            l.iter().map(|c| letter_drop(-10.0, c)).collect::<Vec<_>>(),
            [0.0, 10.0, 0.0, 10.0]
        );
        assert_eq!(
            l.iter().map(|c| letter_drop(0.0, c)).collect::<Vec<_>>(),
            [0.0; 4]
        );
        let mirrored = |rule, n| {
            l.iter()
                .map(|c| letter_mirrored(rule, n, c))
                .collect::<Vec<_>>()
        };
        assert_eq!(mirrored("only", 2), [false, true, false, false]);
        assert_eq!(mirrored("only", 5), [false; 4], "beyond the count");
        assert_eq!(mirrored("every", 1), [true; 4]);
        assert_eq!(mirrored("every", 2), [false, true, false, true]);
        assert_eq!(mirrored("every", 100), [false; 4]);
        assert_eq!(mirrored("firstOfWord", 2), [true, false, true, false]);
        assert_eq!(mirrored("none", 1), [false; 4]);
        // Empty, all spaces, repeated/leading/trailing spaces, other whitespace and a pair.
        assert!(counters("").is_empty());
        assert!(counters("   ").iter().all(Option::is_none));
        let c = counters("  a  b ");
        assert_eq!(c.len(), 7);
        let ab: Vec<_> = c
            .iter()
            .flatten()
            .map(|c| (c.index, c.word_start))
            .collect();
        assert_eq!(ab, [(0, true), (1, true)]);
        let ws = counters("a\u{a0}\t\r\nb\u{301}");
        assert!(ws.iter().all(Option::is_some));
        assert_eq!(
            ws.iter()
                .flatten()
                .map(|c| (c.index, c.word_start))
                .collect::<Vec<_>>(),
            [
                (0, true),
                (1, false),
                (2, false),
                (3, false),
                (4, false),
                (5, false),
                (6, false)
            ]
        );
        let pair = counters("😀 A");
        assert_eq!(pair.len(), 4);
        assert_eq!(
            pair.iter()
                .map(|c| c.map(|c| (c.index, c.word_start, c.phase)))
                .collect::<Vec<_>>(),
            [
                Some((0, true, 1)),
                Some((1, false, 2)),
                None,
                Some((2, true, 3))
            ]
        );
    }

    /// text-formations-text-fun-factory.md Expected results 1: Default (Red) ignores the custom
    /// green; the background is removed unless Maintain background.
    #[test]
    fn expected_1_default_solid_background_removed() {
        let one = run(json!({"solidColor": "red", "customColor": "#00ff00"}));
        assert_px(&one, 44, 80, [255, 0, 0, 255]);
        assert_eq!(px(&one, 395, 295), [0, 0, 0, 0]);
        assert_px(&run(json!({"maintain": true})), 395, 295, [255; 4]);
        // Every Solid source, alpha forced opaque, converted exactly in both contexts.
        let mut env = EffectEnv::whole(W, H);
        env.primary = [10, 200, 30, 128];
        env.secondary = [128, 64, 32, 0];
        let pal = palette_with(&[[1, 2, 3, 4]]);
        env.palette = &pal;
        for (choice, want) in [
            ("primary", [10, 200, 30, 255]),
            ("secondary", [128, 64, 32, 255]),
            ("custom", [0x12, 0x80, 0x56, 255]),
            ("currentPalette", [1, 2, 3, 255]),
            ("defaultPalette", [0, 0, 0, 255]),
        ] {
            let o = run_env(
                json!({"solidColor": choice, "customColor": "#128056"}),
                &env,
            );
            assert_px(&o, 44, 80, want);
        }
        assert_px(
            &run_alt(json!({"solidColor": "custom", "customColor": "#808080"})),
            44,
            80,
            [128, 128, 128, 255],
        );
        assert_px(
            &run(json!({"solidColor": "custom", "customColor": "#808080"})),
            44,
            80,
            [128, 128, 128, 255],
        );
        // Empty or all-space text: transparent, or the source untouched with Maintain.
        let src = from_fn(W, H, |x, y| [x as u8, y as u8, 7, (x + y) as u8]);
        for text in ["", "   "] {
            let out = bb("bbTextFunFactory", base(json!({"text": text})), &src);
            assert!(out.data.iter().all(|&b| b == 0), "{text:?}");
            let kept = bb(
                "bbTextFunFactory",
                base(json!({"text": text, "maintain": true})),
                &src,
            );
            for (x, y) in [(5, 5), (200, 150), (399, 299)] {
                assert_px(&kept, x, y, px(&src, x, y));
            }
        }
        // Maintain composes onto partial and transparent sources; the letter stays opaque.
        for a in [0, 100] {
            let src = solid(W, H, [20, 40, 60, a]);
            let out = bb("bbTextFunFactory", base(json!({"maintain": true})), &src);
            assert_px(&out, 44, 80, [255, 0, 0, 255]);
            assert_eq!(px(&out, 395, 295)[3], a);
        }
    }

    /// text-formations-text-fun-factory.md Expected results 2: Overlap shifts letter k by exactly
    /// k × Overlap; the first letter is on top unless Swap layering direction.
    #[test]
    fn expected_2_overlap_shift_and_layering() {
        let at = |o: i32| {
            placed(json!({"text": "██", "overlap": o}))
                .iter()
                .map(ink_box)
                .collect::<Vec<_>>()
        };
        let (b0, b12, b20) = (at(0), at(-12), at(20));
        assert_eq!(b0[0], b12[0], "the first block never moves");
        assert_eq!(b0[1][0] - b12[1][0], 12.0);
        assert_eq!(b20[1][0] - b0[1][0], 20.0);
        assert_eq!((b0[1][1], b0[1][3]), (b12[1][1], b12[1][3]));
        let pal = palette_with(&[[0, 0, 255, 255], [0, 255, 0, 255]]);
        let env = palette_env(&pal);
        let two = |o: i32, swap: bool, extra: Value| {
            let mut x = json!({"text": "██", "overlap": o, "solidColor": "currentPalette",
                "swapLayering": swap});
            for (k, v) in extra.as_object().unwrap() {
                x[k] = v.clone();
            }
            run_env(x, &env)
        };
        let green = |p: Px| p[..3] == [0, 255, 0];
        let blue = |p: Px| p[..3] == [0, 0, 255];
        // The visible (unoccluded) right edge of the second block moves exactly 12 px.
        let (g0, g12) = (
            bbox(&two(0, false, json!({})), green),
            bbox(&two(-12, false, json!({})), green),
        );
        assert_eq!(g0.2 - g12.2, 12);
        assert_eq!(
            bbox(&two(0, false, json!({})), blue),
            bbox(&two(-12, false, json!({})), blue)
        );
        // In the overlap: entry 0 (first letter) by default, entry 1 swapped.
        let mid = (b12[1][0] as u32 + 2, 80);
        assert_px(&two(-12, false, json!({})), mid.0, mid.1, [0, 0, 255, 255]);
        assert_px(&two(-12, true, json!({})), mid.0, mid.1, [0, 255, 0, 255]);
        // The whole chain stacks: the first letter's shadow lies over the second letter by
        // default and under it when swapped.
        let shadow = json!({"shadowLength": 20, "shadowStrength": 1.0, "light": 0});
        let x = b0[0][2] as u32 + 10;
        let under = two(0, false, shadow.clone());
        assert!(px(&under, x, 80)[1] < 100, "{:?}", px(&under, x, 80));
        assert_px(&two(0, true, shadow), x, 80, [0, 255, 0, 255]);
    }

    /// text-formations-text-fun-factory.md Expected results 3: palette letters skip spaces.
    #[test]
    fn expected_3_palette_letters_skip_spaces() {
        let pal = palette_with(&[[0, 0, 255, 255], [0, 255, 0, 255], [255, 0, 255, 255]]);
        let env = palette_env(&pal);
        for text in ["█ █", " █  █ "] {
            let v = json!({"text": text, "solidColor": "currentPalette"});
            let out = run_env(v.clone(), &env);
            let boxes: Vec<_> = placed(v)
                .iter()
                .filter(|l| !l.paths.is_empty())
                .map(ink_box)
                .collect();
            let centre = |b: [f32; 4]| (((b[0] + b[2]) / 2.0) as u32, 80);
            let (a, b) = (centre(boxes[0]), centre(boxes[1]));
            assert_px(&out, a.0, a.1, [0, 0, 255, 255]);
            assert_px(&out, b.0, b.1, [0, 255, 0, 255]);
            assert!(
                !out.data.chunks(4).any(|q| q[..3] == [255, 0, 255]),
                "{text:?}"
            );
        }
        // A pair is two letters: the block after it takes entry 2 (on top when swapped: the
        // half-pair replacement boxes are wide and the prefix of a whole pair is not).
        let v = json!({"text": "😀█", "solidColor": "currentPalette", "swapLayering": true});
        let b = ink_box(&placed(v.clone())[2]);
        let out = run_env(v, &env);
        assert_px(&out, ((b[0] + b[2]) / 2.0) as u32, 80, [255, 0, 255, 255]);
        // Letter 96 wraps to entry 0; the default palette's half-alpha rows become opaque.
        let stack = |n: usize, choice: &str, env: &EffectEnv| {
            run_env(
                json!({"text": "█".repeat(n), "overlap": -68, "swapLayering": true,
                    "solidColor": choice}),
                env,
            )
        };
        let last = |n: usize| (10.0 + 0.4 * (n - 1) as f32 + 34.2) as u32;
        assert_px(
            &stack(97, "currentPalette", &env),
            last(97),
            80,
            [0, 0, 255, 255],
        );
        let [a, r, g, b] = crate::render::color_ui::DEFAULT_PALETTE[64].to_be_bytes();
        assert_eq!(a, 128);
        assert_px(
            &stack(65, "defaultPalette", &EffectEnv::whole(W, H)),
            last(65),
            80,
            [r, g, b, 255],
        );
    }

    /// The top of each of four blocks (first opaque-ish row at its centre column).
    fn tops(img: &ImageData, n: usize) -> Vec<u32> {
        (0..n)
            .map(|k| {
                let x = (44.2 + 68.4 * k as f32) as u32;
                (0..H).find(|&y| px(img, x, y)[3] > 128).unwrap()
            })
            .collect()
    }

    /// text-formations-text-fun-factory.md Expected results 4: Jumble moves odd (positive) or
    /// even (negative) letters down by exactly |J|; nothing moves up.
    #[test]
    fn expected_4_jumble() {
        let flat = tops(&run(json!({"text": "████"})), 4);
        let up = tops(&run(json!({"text": "████", "jumble": 10})), 4);
        let down = tops(&run(json!({"text": "████", "jumble": -10})), 4);
        let d = |t: &[u32]| {
            (0..4)
                .map(|k| t[k] as i32 - flat[k] as i32)
                .collect::<Vec<_>>()
        };
        assert_eq!(d(&up), [10, 0, 10, 0]);
        assert_eq!(d(&down), [0, 10, 0, 10]);
        // The endpoints, and odd/even continuing across a space.
        for (j, want) in [(50, [50, 0, 50]), (-50, [0, 50, 0])] {
            let l = placed(json!({"text": "█ ██", "jumble": j}));
            let flat = placed(json!({"text": "█ ██"}));
            let drops: Vec<_> = [0, 2, 3]
                .iter()
                .map(|&k| ink_box(&l[k])[1] - ink_box(&flat[k])[1])
                .collect();
            assert_eq!(drops, want.map(|w| w as f32));
        }
    }

    /// text-formations-text-fun-factory.md Expected results 5: "every other letter" turns the first
    /// block 30° clockwise on screen about its own box centre; the second stays upright. (The
    /// file's "top-left corner moves right and down" is the top-right corner: clockwise turns the
    /// top-left corner right and up. spec-question: Q82.)
    #[test]
    fn expected_5_rotate_every_other_letter() {
        let flat = placed(json!({"text": "██"}));
        let turned = placed(json!({"text": "██", "rotate": "everyOther", "rotateAngle": 30}));
        let [l, t, r, b] = ink_box(&flat[0]);
        let (cx, cy) = ((l + r) / 2.0, (t + b) / 2.0);
        let rot = |x: f32, y: f32| {
            let (s, c) = 30f32.to_radians().sin_cos();
            (
                cx + (x - cx) * c - (y - cy) * s,
                cy + (x - cx) * s + (y - cy) * c,
            )
        };
        let tr = rot(r, t);
        assert!(
            tr.0 > r && tr.1 > t,
            "the top-right corner moves right and down"
        );
        let tl = rot(l, t);
        assert!(tl.0 > l && tl.1 < t);
        // The turned first box is the rotated corners' box; the second is untouched.
        let corners = [rot(l, t), rot(r, t), rot(r, b), rot(l, b)];
        let want = [
            corners.iter().map(|p| p.0).fold(f32::MAX, f32::min),
            corners.iter().map(|p| p.1).fold(f32::MAX, f32::min),
            corners.iter().map(|p| p.0).fold(f32::MIN, f32::max),
            corners.iter().map(|p| p.1).fold(f32::MIN, f32::max),
        ];
        let got = ink_box(&turned[0]);
        assert!(
            got.iter().zip(want).all(|(g, w)| (g - w).abs() < 0.01),
            "{got:?}"
        );
        assert_eq!(ink_box(&turned[1]), ink_box(&flat[1]));
        // Rendered: the first block (blue, on top) reaches the rotated top-right corner and
        // leaves its old top-left corner; the second (green) keeps its own corners.
        let pal = palette_with(&[[0, 0, 255, 255], [0, 255, 0, 255]]);
        let env = palette_env(&pal);
        let out = run_env(
            json!({"text": "██", "rotate": "everyOther", "rotateAngle": 30,
                "solidColor": "currentPalette"}),
            &env,
        );
        let inside = rot(r - 6.0, t + 6.0);
        assert_px(&out, inside.0 as u32, inside.1 as u32, [0, 0, 255, 255]);
        assert_eq!(px(&out, l as u32 + 3, t as u32 + 8)[3], 0);
        let second = ink_box(&flat[1]);
        assert_px(
            &out,
            second[2] as u32 - 3,
            second[3] as u32 - 3,
            [0, 255, 0, 255],
        );
        // "back and forth" over four blocks: 0, −30, 0, +30.
        let four = placed(json!({"text": "████", "rotate": "backForth", "rotateAngle": 30}));
        let flat4 = placed(json!({"text": "████"}));
        let widths: Vec<_> = four
            .iter()
            .map(|l| {
                let b = ink_box(l);
                b[2] - b[0]
            })
            .collect();
        let w0 = ink_box(&flat4[0])[2] - ink_box(&flat4[0])[0];
        assert!((widths[0] - w0).abs() < 0.01 && (widths[2] - w0).abs() < 0.01);
        assert!(widths[1] > w0 + 10.0 && (widths[1] - widths[3]).abs() < 0.01);
        // −30 turns the second block the other way: its top-right corner moves left and up.
        let [l1, t1, r1, b1] = ink_box(&flat4[1]);
        let near = |x: f32, y: f32| {
            four[1].paths[0]
                .points()
                .iter()
                .any(|p| (p.x - x).abs() < 0.01 && (p.y - y).abs() < 0.01)
        };
        let (c1x, c1y) = ((l1 + r1) / 2.0, (t1 + b1) / 2.0);
        let (s, c) = (-30f32).to_radians().sin_cos();
        let (dx, dy) = (r1 - c1x, t1 - c1y);
        let moved = (c1x + dx * c - dy * s, c1y + dx * s + dy * c);
        assert!(
            moved.0 < r1 && moved.1 < t1 && near(moved.0, moved.1),
            "{moved:?}"
        );
    }

    /// Mirror, then rotate about the mirrored box centre, then jumble, on an asymmetric F.
    #[test]
    fn transforms_apply_mirror_rotate_jumble_in_order() {
        let v = json!({"text": "F", "reverse": "every", "reverseN": 1, "rotate": "same",
            "rotateAngle": 90, "jumble": 10});
        let l = &placed(v)[0];
        let f = &placed(json!({"text": "F"}))[0];
        let [fl, ft, fr, fb] = ink_box(f);
        let (cx, cy) = ((fl + fr) / 2.0, (ft + fb) / 2.0);
        // F's stem (left) mirrored to the right, then turned 90° clockwise: it ends at the
        // bottom; the box is the turned box, then 10 px lower.
        let got = ink_box(l);
        let hw = (fb - ft) / 2.0;
        let hh = (fr - fl) / 2.0;
        let want = [cx - hw, cy - hh + 10.0, cx + hw, cy + hh + 10.0];
        assert!(
            got.iter().zip(want).all(|(g, w)| (g - w).abs() < 0.05),
            "{got:?}"
        );
        let out = run(
            json!({"text": "F", "reverse": "every", "reverseN": 1, "rotate": "same",
            "rotateAngle": 90, "jumble": 10}),
        );
        // The stem runs along the bottom of the turned box, the arms point up and the empty
        // corner is its top-left (turning first, then mirroring, would leave the stem on top
        // and the bottom-right corner empty).
        let (mx, bottom) = (((want[0] + want[2]) / 2.0) as u32, want[3] as u32 - 3);
        assert_eq!(px(&out, mx, bottom)[3], 255);
        assert_eq!(px(&out, mx, want[1] as u32 + 3)[3], 255, "an arm");
        assert_eq!(px(&out, want[0] as u32 + 3, want[1] as u32 + 3)[3], 0);
        assert_eq!(px(&out, want[2] as u32 - 3, want[1] as u32 + 3)[3], 255);
    }

    /// The Q70 gradient fixture's pixel-centre t on a letter box (vertical style).
    fn vertical_t(b: [f32; 4], y: u32) -> f32 {
        ((y as f32 + 0.5 - b[1]) / (b[3] - b[1])).clamp(0.0, 1.0)
    }
    /// The output byte of a working value in `space`.
    fn encode(v: f32, linear: bool) -> u8 {
        let v = if linear {
            crate::pixel::gamma::linear_to_srgb_f32(v)
        } else {
            v
        };
        crate::pixel::gamma::quantize(v)
    }

    /// text-formations-text-fun-factory.md Expected results 6: the vertical gradient's end rows
    /// are red at the top and blue at the bottom (±4 on their dominant channel); every sampled
    /// row is the Q70 raw-working value at its own pixel-centre t (±2), so the midpoint is about
    /// 188 checked and 223 unchecked (native Linear), 128 / 188 in the encoded context.
    // spec-question: Q81 Text Fun Factory gradient end rows under raw-working stops
    #[test]
    fn expected_6_vertical_gradient() {
        let b = ink_box(&placed(json!({}))[0]);
        let x = ((b[0] + b[2]) / 2.0) as u32;
        let (top, bottom) = (b[1].ceil() as u32 + 1, b[3].floor() as u32 - 2);
        for srgb in [true, false] {
            for (out, linear) in [
                (run(json!({"fillTab": 1, "srgbGradient": srgb})), true),
                (run_alt(json!({"fillTab": 1, "srgbGradient": srgb})), false),
            ] {
                let p = px(&out, x, top);
                assert!(p[0].abs_diff(255) <= 4 && p[1] == 0 && p[3] == 255, "{p:?}");
                let p = px(&out, x, bottom);
                assert!(p[2].abs_diff(255) <= 4 && p[1] == 0 && p[3] == 255, "{p:?}");
                for y in [top, bottom, 45, 60, 80, 100, 120] {
                    let t = vertical_t(b, y);
                    let g = gradient_raw_working([255, 0, 0], [0, 0, 255], t, srgb);
                    let want = [encode(g[0], linear), 0, encode(g[2], linear), 255];
                    assert_px(&out, x, y, want);
                }
            }
        }
        // Mid-height, the row whose centre is nearest t = .5.
        let mid = ((b[1] + b[3]) / 2.0 - 0.5).round() as u32;
        let near = |out: &ImageData, want: u8| {
            let p = px(out, x, mid);
            assert!(
                p[0].abs_diff(want) <= 2 && p[2].abs_diff(want) <= 2,
                "{p:?} vs {want}"
            );
        };
        near(&run(json!({"fillTab": 1})), 188);
        near(&run(json!({"fillTab": 1, "srgbGradient": false})), 223);
        near(&run_alt(json!({"fillTab": 1})), 128);
        near(&run_alt(json!({"fillTab": 1, "srgbGradient": false})), 188);
    }

    /// R158 Q70: the raw-working stage at exact t, both states, against the follow-up's table.
    #[test]
    fn gradient_raw_working_table() {
        let out = |g: [f32; 3], linear: bool| g.map(|v| encode(v, linear));
        let rb = |t: f32, srgb| gradient_raw_working([255, 0, 0], [0, 0, 255], t, srgb);
        for (t, cl, ca, ul, ua) in [
            (0.0, [255, 0, 0], [255, 0, 0], [255, 0, 0], [255, 0, 0]),
            (
                0.25,
                [225, 0, 137],
                [191, 0, 64],
                [241, 0, 194],
                [225, 0, 137],
            ),
            (
                0.5,
                [188, 0, 188],
                [128, 0, 128],
                [223, 0, 223],
                [188, 0, 188],
            ),
            (
                0.75,
                [137, 0, 225],
                [64, 0, 191],
                [194, 0, 241],
                [137, 0, 225],
            ),
            (1.0, [0, 0, 255], [0, 0, 255], [0, 0, 255], [0, 0, 255]),
        ] {
            for (got, want) in [
                (out(rb(t, true), true), cl),
                (out(rb(t, true), false), ca),
                (out(rb(t, false), true), ul),
                (out(rb(t, false), false), ua),
            ] {
                assert!(
                    got.iter()
                        .zip(want)
                        .all(|(g, w): (&u8, u8)| g.abs_diff(w) <= 1),
                    "t {t}: {got:?} vs {want:?}"
                );
            }
        }
        // Constant 128 stays raw 128/255 (188 Linear, 128 encoded); exact endpoints; clamping.
        for t in [0.0, 0.3, 1.0] {
            for srgb in [true, false] {
                let g = gradient_raw_working([128; 3], [128; 3], t, srgb);
                assert!(g.iter().all(|&v| (v - 128.0 / 255.0).abs() < 1e-6), "{g:?}");
            }
        }
        assert_eq!(
            gradient_raw_working([64, 96, 128], [192, 160, 32], 0.0, false),
            [64.0 / 255.0, 96.0 / 255.0, 128.0 / 255.0]
        );
        assert_eq!(
            gradient_raw_working([64, 96, 128], [192, 160, 32], 2.0, true),
            [192.0 / 255.0, 160.0 / 255.0, 32.0 / 255.0]
        );
        let mid = |srgb, linear| {
            out(
                gradient_raw_working([64, 96, 128], [192, 160, 32], 0.5, srgb),
                linear,
            )
        };
        for (got, want) in [
            (mid(true, true), [188, 188, 152]),
            (mid(true, false), [128, 128, 80]),
            (mid(false, true), [199, 191, 164]),
            (mid(false, false), [146, 133, 95]),
        ] {
            assert!(
                got.iter()
                    .zip(want)
                    .all(|(g, w): (&u8, u8)| g.abs_diff(w) <= 1),
                "{got:?} vs {want:?}"
            );
        }
    }

    /// All four gradient axes over each letter's own final box, restarting per letter.
    #[test]
    fn gradient_styles_follow_each_letter_box() {
        // Turned and jumbled letters: each gradient spans that letter's own final box.
        let v = json!({"text": "██", "jumble": 20, "rotate": "everyOther", "rotateAngle": 20});
        let boxes: Vec<_> = placed(v.clone()).iter().map(ink_box).collect();
        for (style, axis) in [
            ("vertical", [0.0, 0.0, 0.0, 1.0]),
            ("horizontal", [0.0, 0.0, 1.0, 0.0]),
            ("topLeft", [0.0, 0.0, 1.0, 1.0]),
            ("topRight", [1.0, 0.0, 0.0, 1.0]),
        ] {
            let mut x = v.clone();
            x["fillTab"] = json!(1);
            x["gradStyle"] = json!(style);
            let out = run(x);
            for b in &boxes {
                let (w, h) = (b[2] - b[0], b[3] - b[1]);
                let s = [b[0] + axis[0] * w, b[1] + axis[1] * h];
                let e = [b[0] + axis[2] * w, b[1] + axis[3] * h];
                // Interior points of the upright second block and the turned first one.
                for (fx, fy) in [(0.5, 0.5), (0.3, 0.3), (0.7, 0.6), (0.5, 0.2)] {
                    let (x, y) = ((b[0] + w * fx) as u32, (b[1] + h * fy) as u32);
                    if px(&out, x, y)[3] != 255 || px(&out, x + 1, y)[3] != 255 {
                        continue;
                    }
                    let t = gradient_t([x as f32 + 0.5, y as f32 + 0.5], s, e);
                    let g = gradient_raw_working([255, 0, 0], [0, 0, 255], t, true);
                    assert_px(&out, x, y, [encode(g[0], true), 0, encode(g[2], true), 255]);
                }
            }
        }
        // Projection: off the axis a point takes its foot's t, clamped past both ends; a
        // degenerate axis is all From.
        assert_eq!(gradient_t([5.0, 9.0], [0.0, 0.0], [10.0, 0.0]), 0.5);
        assert_eq!(gradient_t([-5.0, 1.0], [0.0, 0.0], [10.0, 0.0]), 0.0);
        assert_eq!(gradient_t([15.0, 1.0], [0.0, 0.0], [10.0, 0.0]), 1.0);
        assert_eq!(gradient_t([5.0, 5.0], [1.0, 2.0], [1.0, 2.0]), 0.0);
    }

    /// Checks every fully covered pixel whose four neighbours are covered too (no edge blend) is
    /// `f(x, y)`; returns how many there were.
    fn covered(img: &ImageData, f: &dyn Fn(u32, u32) -> Px) -> usize {
        let mut n = 0;
        for y in 1..H - 1 {
            for x in 0..W {
                let all =
                    [(0i32, 0i32), (0, -1), (0, 1), (-1, 0), (1, 0)]
                        .iter()
                        .all(|&(dx, dy)| {
                            let (a, b) = ((x as i32 + dx) as u32, (y as i32 + dy) as u32);
                            a < W && px(img, a, b)[3] == 255
                        });
                if all {
                    assert_px(img, x, y, f(x, y));
                    n += 1;
                }
            }
        }
        n
    }
    /// The Image sub-tab at identity (angle 90, scale 1, no mirror, Hue 0, Saturation 100) on
    /// `texture`, then `extra`.
    fn image_fill(texture: &str, extra: Value) -> Value {
        let mut x = json!({"fillTab": 3, "texture": texture, "textureAngle": 90, "scale": 1,
            "mirror": false, "textureHue": 0, "textureSat": 100});
        for (k, v) in extra.as_object().unwrap() {
            x[k] = v.clone();
        }
        x
    }

    /// text-formations-text-fun-factory.md Expected results 7: the texture is anchored to the
    /// canvas: every fully covered pixel is Check(x mod 278, y mod 278), wherever the text,
    /// selection or letter turn is; angle 0 turns it 90° clockwise.
    #[test]
    fn expected_7_texture_anchored_to_canvas() {
        let check = super::super::textures::texture(47);
        let tex = |x: u32, y: u32| px(&check, x % 278, y % 278);
        let fill = |extra: Value| image_fill("t47", extra);
        let mut n = covered(&run(fill(json!({}))), &tex);
        n += covered(&run(fill(json!({"offsetX": 30}))), &tex);
        n += covered(
            &run(fill(json!({"rotate": "same", "rotateAngle": 30}))),
            &tex,
        );
        // Across both 278 boundaries.
        n += covered(
            &run(fill(json!({"text": "████", "size": 200, "offsetY": 50}))),
            &tex,
        );
        let (mask, polys) = rect_selection(W, H, 50, 40, 400, 300);
        let env = with_selection(W, H, &mask, &polys);
        let clear = solid(W, H, [0; 4]);
        n += covered(
            &bb_env("bbTextFunFactory", base(fill(json!({}))), &clear, &env),
            &tex,
        );
        assert!(n > 60_000, "{n}");
        // Angle 0: canvas (x, y) shows texel (y, 277 − x).
        let turned = |x: u32, y: u32| px(&check, y % 278, 277 - x % 278);
        assert!(covered(&run(fill(json!({"textureAngle": 0}))), &turned) > 5000);
        // Without a clipboard image the clipboard entry takes the Solid fill (Default red).
        assert_px(
            &run(fill(json!({"texture": "t49"}))),
            44,
            80,
            [255, 0, 0, 255],
        );
    }

    /// text-formations-text-fun-factory.md Expected results 7 with "Use Image on Clipboard"
    /// (BB33): the clipboard image tiles at its own size from the canvas origin (never a
    /// 278-square tile): every fully covered pixel is image(x mod w, y mod h) for nonsquare,
    /// 1 × N and N × 1 images, wherever the text or the selection is, across many tile
    /// boundaries; angle 0 shows image(y mod w, h − 1 − x mod h).
    #[test]
    fn expected_7_clipboard_image_at_its_own_size() {
        let fill = |extra: Value| image_fill("t49", extra);
        for (w, h) in [(5, 3), (1, 4), (4, 1)] {
            let img = from_fn(w, h, |x, y| {
                [
                    (30 + 40 * x) as u8,
                    (50 + 60 * y) as u8,
                    (90 + 25 * (x + y)) as u8,
                    255,
                ]
            });
            let at = |x: u32, y: u32| px(&img, x % w, y % h);
            let env = EffectEnv {
                image: Some(&img),
                ..EffectEnv::whole(W, H)
            };
            let mut n = covered(
                &run_env(
                    fill(json!({"text": "████", "size": 200, "offsetY": 50})),
                    &env,
                ),
                &at,
            );
            n += covered(&run_env(fill(json!({"offsetX": 30})), &env), &at);
            let (mask, polys) = rect_selection(W, H, 50, 40, 400, 300);
            let selected = EffectEnv {
                image: Some(&img),
                ..with_selection(W, H, &mask, &polys)
            };
            let clear = solid(W, H, [0; 4]);
            n += covered(
                &bb_env("bbTextFunFactory", base(fill(json!({}))), &clear, &selected),
                &at,
            );
            assert!(n > 60_000, "{w}×{h}: {n}");
            let turned = |x: u32, y: u32| px(&img, y % w, h - 1 - x % h);
            let m = covered(&run_env(fill(json!({"textureAngle": 0})), &env), &turned);
            assert!(m > 5000, "{w}×{h}: {m}");
        }
    }

    /// R158 / BB33: the clipboard image takes the built-in textures' own brush: with the Check
    /// texture's pixels on the clipboard, "Use Image on Clipboard" renders byte for byte what
    /// "Check" does, under turns, scales, mirroring, Hue / Saturation (applied once, also above
    /// 100), a maintained background, bevel, outline and shadow, a fractional selection edge
    /// (clipped once, the outside untouched) and both effective contexts.
    #[test]
    fn clipboard_image_takes_the_built_in_brush() {
        let check = super::super::textures::texture(47);
        let def = find("bbTextFunFactory").unwrap();
        let EffectKind::Gpu(_, render) = def.kind else {
            unreachable!()
        };
        let (mask, polys) = ellipse_selection(W, H, [200.0, 150.0], [150.5, 90.25]);
        for extra in [
            json!({}),
            json!({"textureAngle": 0, "mirror": true}),
            json!({"textureAngle": 180, "scale": 2.3}),
            json!({"textureAngle": 30, "scale": 0.5, "mirror": true, "textureHue": 40,
                "textureSat": 150}),
            json!({"textureHue": -90, "textureSat": 30, "maintain": true, "depth": 4,
                "outlineSize": 2, "shadowLength": 5}),
        ] {
            for info in [GPU_CONTEXT, ALT_GPU_CONTEXT] {
                for selected in [false, true] {
                    let def = EffectDef {
                        kind: EffectKind::Gpu(info, render),
                        ..def.clone()
                    };
                    let out = |texture: &str, image: Option<&ImageData>| {
                        let mut env = if selected {
                            with_selection(W, H, &mask, &polys)
                        } else {
                            EffectEnv::whole(W, H)
                        };
                        env.image = image;
                        let v = values(image_fill(texture, extra.clone()));
                        crate::effect::apply(&white(), &def, &v, &env)
                    };
                    let clipboard = out("t49", Some(&check));
                    assert!(clipboard == out("t47", None), "{extra} {info:?} {selected}");
                    if selected {
                        for (x, y) in [(10, 10), (399, 299), (200, 55)] {
                            assert_eq!(px(&clipboard, x, y), [255; 4]);
                        }
                    }
                }
            }
        }
    }

    /// BB33: the clipboard image keeps its own alpha (straight sRGB bytes): over transparent a
    /// translucent texel keeps its colour and alpha; with Maintain background it blends onto the
    /// white source in linear light. A fully transparent image is still an image, so nothing is
    /// drawn, not even its shadow (not the Solid fallback's red, not its hidden RGB).
    #[test]
    fn clipboard_image_alpha_is_kept() {
        let texels = [
            [200, 100, 50, 255],
            [40, 160, 220, 128],
            [90, 90, 90, 0],
            [250, 20, 120, 64],
        ];
        let img = from_fn(2, 2, |x, y| texels[(y * 2 + x) as usize]);
        let env = EffectEnv {
            image: Some(&img),
            ..EffectEnv::whole(W, H)
        };
        // Strength 0: the always-applied shadow adds no alpha of its own here.
        let big = json!({"text": "████", "size": 200, "offsetY": 50, "shadowStrength": 0});
        let clear = run_env(image_fill("t49", big.clone()), &env);
        let mut kept = big.clone();
        kept["maintain"] = json!(true);
        let kept = run_env(image_fill("t49", kept), &env);
        let lin = |c: u8| srgb_to_linear_f32(c as f32 / 255.0);
        for y in 150..154 {
            for x in 100..104 {
                let t = px(&img, x % 2, y % 2);
                let a = t[3] as f32 / 255.0;
                if t[3] == 0 {
                    assert_eq!(px(&clear, x, y)[3], 0);
                } else {
                    assert_px(&clear, x, y, t);
                }
                let over = |c: u8| (linear_to_srgb_f32(lin(c) * a + 1.0 - a) * 255.0).round() as u8;
                assert_px(&kept, x, y, [over(t[0]), over(t[1]), over(t[2]), 255]);
            }
        }
        // The cubic taps are premultiplied: a transparent texel's hidden red never fringes.
        let hidden = from_fn(2, 1, |x, _| [[255, 0, 0, 0], [0, 0, 255, 255]][x as usize]);
        let env = EffectEnv {
            image: Some(&hidden),
            ..EffectEnv::whole(W, H)
        };
        let mut turned = big.clone();
        turned["scale"] = json!(2.5);
        turned["textureAngle"] = json!(30);
        let out = run_env(image_fill("t49", turned), &env);
        assert!(out.data.chunks(4).all(|p| p[0] <= 2));
        assert!(
            out.data
                .chunks(4)
                .any(|p| p[3] > 0 && p[3] < 255 && p[2] > 200)
        );
        let transparent = solid(3, 2, [255, 0, 0, 0]);
        let env = EffectEnv {
            image: Some(&transparent),
            ..EffectEnv::whole(W, H)
        };
        let out = run_env(image_fill("t49", json!({})), &env);
        assert!(out.data.chunks(4).all(|p| p[3] == 0));
        assert!(run_env(image_fill("t49", json!({"maintain": true})), &env) == white());
    }

    /// text-formations-text-fun-factory.md "Image" ("If the clipboard holds no image, the letters
    /// get the Solid fill"), BB33: with no clipboard image (or an empty one) each of the six
    /// Solid choices renders exactly as on the Solid sub-tab, Expected results 1's red despite a
    /// green Custom colour and Expected results 3's palette letters skipping spaces included; the
    /// Image sub-tab's turn, scale, mirror and Hue / Saturation do not touch it, in either
    /// context.
    #[test]
    fn clipboard_without_an_image_takes_the_solid_fill() {
        let pal = palette_with(&[[10, 200, 30, 77], [220, 40, 160, 255]]);
        let empty = ImageData {
            width: 0,
            height: 0,
            data: vec![],
        };
        let def = find("bbTextFunFactory").unwrap();
        let EffectKind::Gpu(_, render) = def.kind else {
            unreachable!()
        };
        for info in [GPU_CONTEXT, ALT_GPU_CONTEXT] {
            let def = EffectDef {
                kind: EffectKind::Gpu(info, render),
                ..def.clone()
            };
            for choice in [
                "red",
                "primary",
                "secondary",
                "custom",
                "currentPalette",
                "defaultPalette",
            ] {
                let out = |fill_tab: u32, image: Option<&ImageData>| {
                    let env = EffectEnv {
                        image,
                        primary: [12, 34, 56, 100],
                        secondary: [200, 150, 100, 7],
                        palette: &pal,
                        ..EffectEnv::whole(W, H)
                    };
                    let v = values(image_fill(
                        "t49",
                        json!({"fillTab": fill_tab, "text": "█ █", "solidColor": choice,
                            "customColor": "#00ff00", "textureAngle": 30, "scale": 2,
                            "mirror": true, "textureHue": 90, "textureSat": 150}),
                    ));
                    crate::effect::apply(&white(), &def, &v, &env)
                };
                let solid_tab = out(0, None);
                for image in [None, Some(&empty)] {
                    assert!(out(3, image) == solid_tab, "{choice} {info:?}");
                }
                if info == GPU_CONTEXT {
                    let (first, second) = match choice {
                        "red" => ([255, 0, 0, 255], [255, 0, 0, 255]),
                        "currentPalette" => ([10, 200, 30, 255], [220, 40, 160, 255]),
                        _ => continue,
                    };
                    assert_px(&solid_tab, 44, 80, first);
                    let right = bbox(&solid_tab, |p| p[3] > 0).2;
                    assert_px(&solid_tab, right - 20, 80, second);
                }
            }
        }
    }

    /// BB33: only the effective Image fill reads the clipboard: a remembered "Use Image on
    /// Clipboard" on Solid, Gradient or Metallic, and a built-in texture, ignore the image.
    #[test]
    fn only_the_image_fill_reads_the_clipboard() {
        let img = solid(3, 2, [10, 220, 90, 255]);
        let with = EffectEnv {
            image: Some(&img),
            ..EffectEnv::whole(W, H)
        };
        for extra in [
            json!({"fillTab": 0}),
            json!({"fillTab": 1}),
            json!({"fillTab": 2}),
            json!({"fillTab": 2, "metal": "gold"}),
            json!({"texture": "t5"}),
        ] {
            let x = image_fill("t49", extra.clone());
            assert!(run_env(x.clone(), &with) == run(x), "{extra}");
        }
    }

    /// R155 / BB33: the dialog and Repeat ask for the clipboard only for the effective Image
    /// fill on "Use Image on Clipboard", whichever outer tab shows.
    #[test]
    fn clipboard_is_read_only_for_the_image_fill() {
        use super::super::ui::External;
        let read = |extra: Value| (UI.external_for)(&values(extra));
        assert_eq!(read(json!({})), External::None);
        for tab in 0..6 {
            assert_eq!(
                read(image_fill("t49", json!({"tab": tab}))),
                External::Clipboard
            );
        }
        for extra in [
            json!({"fillTab": 0}),
            json!({"fillTab": 1}),
            json!({"fillTab": 2}),
            json!({"texture": "t0"}),
            json!({"texture": "t48"}),
            json!({"texture": "t50"}),
        ] {
            assert_eq!(
                read(image_fill("t49", extra.clone())),
                External::None,
                "{extra}"
            );
        }
    }

    /// R158 brush: canvas-anchored inverse, Wrap/Mirror on the source's own size with negative
    /// taps, Catmull-Rom between texels, Hue/Saturation once in the working space.
    #[test]
    fn brush_sampling_and_hue_saturation() {
        let src = from_fn(3, 2, |x, y| [40 * x as u8 + 10, 100 * y as u8 + 20, 7, 255]);
        let info = GPU_CONTEXT;
        let exact = |x: u32, y: u32| {
            crate::effect::surface::EffectSurface::from_rgba8(&src, info.space, info.input).at(x, y)
        };
        let wrap = Brush::new(&src, 0.0, 100.0, 90.0, 1.0, false, info);
        let mirror = Brush::new(&src, 0.0, 100.0, 90.0, 1.0, true, info);
        for (x, y, wx, wy, mx, my) in [
            (0, 0, 0, 0, 0, 0),
            (2, 1, 2, 1, 2, 1),
            (-1, 0, 2, 0, 0, 0),
            (-2, -1, 1, 1, 1, 0),
            (3, 2, 0, 0, 2, 1),
            (5, 3, 2, 1, 0, 0),
        ] {
            assert_eq!(wrap.at(x, y), exact(wx, wy), "wrap {x},{y}");
            assert_eq!(mirror.at(x, y), exact(mx, my), "mirror {x},{y}");
        }
        // Scale 2: the sample point is p / 2, filtered with Catmull-Rom taps.
        let half = Brush::new(&src, 0.0, 100.0, 90.0, 2.0, false, info);
        let surface =
            crate::effect::surface::EffectSurface::from_rgba8(&src, info.space, info.input);
        let want = crate::effect::distort::filter4(
            [1.75, 0.75],
            crate::effect::distort::catmull_rom,
            |i, j| surface.fetch(i, j, crate::effect::surface::Edge::Wrap),
        );
        assert_eq!(half.at(3, 1), want);
        // Angle 0 (turned 90° clockwise): canvas (x, y) samples source (y, −1 − x).
        let quarter = Brush::new(&src, 0.0, 100.0, 0.0, 1.0, false, info);
        assert_eq!(quarter.at(0, 1), exact(1, 1));
        assert_eq!(quarter.at(1, 0), exact(0, 0));
        // A noncardinal angle samples between texels.
        let odd = Brush::new(&src, 0.0, 100.0, 60.0, 1.0, false, info);
        assert_ne!(odd.at(1, 1), exact(1, 1));
        // Saturation factor = slider / 100 (R158), not the shared helper's percent.
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5);
        let rgb = [0.6, 0.4, 0.4];
        for (s, want) in [
            (0.0, [0.5, 0.5, 0.5]),
            (100.0, [0.6, 0.4, 0.4]),
            (150.0, [0.65, 0.35, 0.35]),
            (200.0, [0.7, 0.3, 0.3]),
        ] {
            assert!(close(hue_saturation(rgb, 0.0, s), want), "{s}");
        }
        assert!(close(
            crate::effect::bb::blocks::hsl_adjust(rgb, 0.0, 150.0, 0.0),
            [0.75, 0.25, 0.25]
        ));
        assert!(close(
            hue_saturation([1.0, 0.0, 0.0], 120.0, 100.0),
            [0.0, 1.0, 0.0]
        ));
        assert!(close(
            hue_saturation([1.0, 0.0, 0.0], 180.0, 100.0),
            [0.0, 1.0, 1.0]
        ));
        // The rendered texture with saturation 0 is grey.
        let grey = run(json!({"fillTab": 3, "texture": "t47", "textureSat": 0}));
        let p = px(&grey, 44, 80);
        assert!(p[0] == p[1] && p[1] == p[2], "{p:?}");
    }

    /// text-formations-text-fun-factory.md Expected results 8: a 20 px shadow to the right, half
    /// strength 20 px out, rising closer, falling beyond, nothing on the left; default −45° falls
    /// down-right by (14.1, 14.1).
    #[test]
    fn expected_8_shadow_direction_and_half_point() {
        let sh = run(json!({"shadowLength": 20, "shadowStrength": 1.0, "light": 0}));
        let block = bbox(&sh, |p| p[..3] == [255, 0, 0] && p[3] == 255);
        let my = (block.1 + block.3) / 2;
        // Measured from the block's (fractional) right edge, 78.4.
        let right = ink_box(&placed(json!({}))[0])[2];
        let a = |dx: u32| px(&sh, (right + dx as f32) as u32, my);
        assert!(
            a(20)[3].abs_diff(128) <= 6 && a(20)[0] <= TOL,
            "{:?}",
            a(20)
        );
        assert!(a(10)[3] > a(20)[3] && a(25)[3] < a(20)[3]);
        // The σ = radius/3 inference: about 238 at 10 px and 58 at 25 px.
        assert!(
            a(10)[3].abs_diff(238) <= 6 && a(25)[3].abs_diff(58) <= 6,
            "{:?} {:?}",
            a(10),
            a(25)
        );
        assert_eq!(px(&sh, block.0 - 10, my)[3], 0);
        let [dx, dy] = crate::effect::bb::blocks::shadow_offset(
            20.0,
            -45.0 + super::super::conventions::SHADOW_ANGLE_OFFSET_DEG,
        );
        assert!((dx - 14.142).abs() < 0.01 && (dy - 14.142).abs() < 0.01);
        // A raw coloured shadow at strength .75 (191/255), well inside its silhouette.
        let grey = run(
            json!({"shadowLength": 20, "shadowStrength": 0.75, "light": 0,
            "shadowColor": "#808080"}),
        );
        assert_px(&grey, block.2 + 2, my, [188, 188, 188, 191]);
        // The outline is part of the silhouette.
        let rim = run(
            json!({"shadowLength": 20, "shadowStrength": 1.0, "light": 0,
            "outlineSize": 10}),
        );
        assert!(px(&rim, block.2 + 25, my)[3] > a(25)[3]);
        // Length 0 still applies it: an edge pixel of coverage a becomes a + a·s·(1 − a).
        let bare = px(
            &run(json!({"shadowLength": 0, "shadowStrength": 0.0})),
            78,
            80,
        )[3];
        let edge = px(
            &run(json!({"shadowLength": 0, "shadowStrength": 1.0})),
            78,
            80,
        )[3];
        let a = bare as f32 / 255.0;
        assert!(bare > 0 && bare < 255);
        assert!(
            (edge as f32 - 255.0 * (a + a * (1.0 - a))).abs() <= 1.0,
            "{bare} {edge}"
        );
    }

    /// R158 halo: a larger canvas with the same text over its selection gives the same pixels:
    /// letters and outlines off the layer still cast their shadows inward.
    #[test]
    fn off_canvas_letters_cast_inward() {
        // The reference's margin (160) exceeds the largest reach (len 50: 100 + 36 + 2).
        let (w, h, m) = (120, 90, 160);
        let big = solid(w + 2 * m, h + 2 * m, [255; 4]);
        let (mask, polys) = rect_selection(w + 2 * m, h + 2 * m, m, m, w + m, h + m);
        let env = with_selection(w + 2 * m, h + 2 * m, &mask, &polys);
        // The second block lies wholly past the right edge; swapped on top, its shadow falls
        // left onto the first.
        let past = json!({"text": "██", "offsetX": 50, "swapLayering": true, "shadowLength": 40,
            "light": 180});
        for (len, light, extra) in [
            (40, 180.0, past.clone()),
            (20, 0.0, json!({"offsetX": -50, "outlineSize": 6})),
            (20, -90.0, json!({"offsetY": -50})),
            (20, 90.0, json!({"offsetY": 30})),
            (50, 135.0, json!({"offsetX": -50, "offsetY": -50})),
            (0, 0.0, json!({"offsetX": -50})),
        ] {
            let mut v = base(extra);
            v["shadowLength"] = json!(len);
            v["light"] = json!(light);
            v["shadowStrength"] = json!(1.0);
            let small = bb("bbTextFunFactory", v.clone(), &solid(w, h, [255; 4]));
            let reference = bb_env("bbTextFunFactory", v, &big, &env);
            for y in 0..h {
                for x in 0..w {
                    assert_px(&small, x, y, px(&reference, x + m, y + m));
                }
            }
        }
        // The first case really draws the off-layer letter's shadow.
        let canvas = solid(w, h, [255; 4]);
        let mut one = base(past.clone());
        one["text"] = json!("█");
        let (a, b) = (
            bb("bbTextFunFactory", base(past), &canvas),
            bb("bbTextFunFactory", one, &canvas),
        );
        assert!(
            px(&a, 115, 60)[0] < px(&b, 115, 60)[0],
            "{:?}",
            px(&a, 115, 60)
        );
    }

    /// text-formations-text-fun-factory.md Expected results 11: the outline colour is raw (#808080
    /// shows about 188 in the linear context, 128 in the encoded one); black and white are exact.
    #[test]
    fn expected_11_outline_colour_is_raw() {
        let ring = run(json!({"outlineSize": 4, "outlineColor": "#808080", "outlineOnTop": true}));
        let b = bbox(&ring, |p| p[3] > 0);
        let (x, y) = (b.0 + 2, (b.1 + b.3) / 2);
        assert_px(&ring, x, y, [188, 188, 188, 255]);
        for (c, v) in [("#000000", 0), ("#ffffff", 255)] {
            let o = run(json!({"outlineSize": 4, "outlineColor": c, "outlineOnTop": true}));
            assert_eq!(px(&o, x, y), [v, v, v, 255], "{c}");
        }
        let alt =
            run_alt(json!({"outlineSize": 4, "outlineColor": "#808080", "outlineOnTop": true}));
        assert_px(&alt, x, y, [128, 128, 128, 255]);
        // Centred: 2 px outside the 10.0 edge; under the fill only the outer half shows.
        assert_eq!(b.0, 8);
        let under = run(json!({"outlineSize": 4, "outlineColor": "#000000"}));
        assert_eq!(px(&under, 8, y), [0, 0, 0, 255]);
        assert_px(&under, 11, y, [255, 0, 0, 255]);
        let over = run(json!({"outlineSize": 4, "outlineColor": "#000000", "outlineOnTop": true}));
        assert_eq!(px(&over, 11, y), [0, 0, 0, 255]);
        assert_px(&over, 13, y, [255, 0, 0, 255]);
        // Size 0 draws none.
        assert_eq!(
            px(
                &run(json!({"outlineSize": 0, "outlineColor": "#000000"})),
                8,
                y
            )[3],
            0
        );
        // Joins at the block's top-left corner (10, 10.944), width 10: pixel (5, 6) lies inside
        // the square miter only (more than 5 px from the corner, |dx| + |dy| > 5).
        let corner = |join: &str| {
            let o = run(json!({"outlineSize": 10, "outlineColor": "#000000", "corner": join}));
            px(&o, 5, 6)[3]
        };
        for j in ["miter", "default", "miterOrBevel"] {
            assert!(corner(j) >= 250, "{j}");
        }
        assert_eq!(corner("round"), 0);
        assert_eq!(corner("bevel"), 0);
    }

    /// Default = Miter (clipped at limit 10); Miter or Bevel falls back to a bevel past it.
    #[test]
    fn acute_joins_follow_the_miter_limit() {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(10.0, 50.0);
        pb.line_to(90.0, 46.0);
        pb.line_to(10.0, 42.0);
        pb.close();
        let spike = pb.finish().unwrap();
        let cover = |join: &str| {
            let p = super::super::raster::widen(&spike, &outline_pen(join, 4.0)).unwrap();
            super::super::text::coverage(&[p], None, 140, 100)[46 * 140 + 95]
        };
        assert_eq!(cover("default"), cover("miter"));
        assert!(cover("miter") > 0.5);
        assert_eq!(cover("miterOrBevel"), 0.0);
    }

    /// Winding per item: O keeps its hole, and synthetic bold's overlapping stroke and base
    /// items never cancel (Noto Sans Mono has no bold face: R111 thickening).
    #[test]
    fn winding_keeps_holes_and_overlaps() {
        for bold in [false, true] {
            let o = run(json!({"text": "O", "bold": bold}));
            let b = bbox(&o, |p| p[3] > 0);
            let (cx, cy) = ((b.0 + b.2) / 2, (b.1 + b.3) / 2);
            assert_eq!(px(&o, cx, cy)[3], 0, "bold {bold}");
            assert_px(&o, b.0 + 3, cy, [255, 0, 0, 255]);
        }
        let l = placed(json!({"text": "O", "bold": true}));
        assert_eq!(l[0].paths.len(), 2, "stroke item and base item");
    }

    /// text-formations-text-fun-factory.md Expected results 12: "only letter number: 2" mirrors the
    /// second F inside its own box and leaves the first untouched; "every 1" mirrors both.
    #[test]
    fn expected_12_reverse() {
        let plain = run(json!({"text": "FF"}));
        let second = run(json!({"text": "FF", "reverse": "only", "reverseN": 2}));
        let both = run(json!({"text": "FF", "reverse": "every", "reverseN": 1}));
        // The first F's region is byte-identical.
        for y in 0..H {
            for x in 0..79 {
                assert_eq!(px(&plain, x, y), px(&second, x, y));
            }
        }
        let f = placed(json!({"text": "FF"}));
        let boxes: Vec<_> = f.iter().map(ink_box).collect();
        let r = placed(json!({"text": "FF", "reverse": "only", "reverseN": 2}));
        assert_eq!(ink_box(&r[0]), boxes[0]);
        assert_eq!(ink_box(&r[1]), boxes[1], "mirrored within its own box");
        // F's stem is at the left near the baseline; mirrored, at the right.
        let y = 112;
        let (l0, r0) = (boxes[0][0] as u32 + 3, boxes[0][2] as u32 - 3);
        let (l1, r1) = (boxes[1][0] as u32 + 3, boxes[1][2] as u32 - 3);
        assert_eq!((px(&plain, l1, y)[3], px(&plain, r1, y)[3]), (255, 0));
        assert_eq!((px(&second, l1, y)[3], px(&second, r1, y)[3]), (0, 255));
        assert_eq!((px(&both, l0, y)[3], px(&both, r0, y)[3]), (0, 255));
        assert_eq!((px(&both, l1, y)[3], px(&both, r1, y)[3]), (0, 255));
        // First of each word; spaces are not counted.
        let w = placed(json!({"text": "FF F", "reverse": "firstOfWord"}));
        let p = placed(json!({"text": "FF F"}));
        let mirrored = |k: usize| {
            let (a, b) = (&w[k].paths[0], &p[k].paths[0]);
            a.points().first() != b.points().first()
        };
        assert_eq!((mirrored(0), mirrored(1), mirrored(3)), (true, false, true));
    }

    /// R158 Q68: each letter's box starts at the whole raw prefix's measured width (trailing
    /// spaces in) plus k × Overlap; a half pair measures U+FFFD, a whole pair decodes; CR, LF,
    /// tab, NBSP and selectors stay units on one line.
    #[test]
    fn raw_prefixes_place_each_unit() {
        let width = |units: &[u16]| whole(units, FONT, false);
        for text in [
            "A B",
            "A  B",
            " A ",
            "😀A",
            "A😀B",
            "A\nB\r\tC\u{a0}D",
            "☺\u{fe0f}A",
            "AV",
        ] {
            let units: Vec<u16> = text.encode_utf16().collect();
            let l = placed(json!({"text": text, "overlap": 3}));
            assert_eq!(l.len(), units.len(), "{text:?}");
            for (k, letter) in l.iter().enumerate() {
                let want = 10.0 + width(&units[..k]) + 3.0 * k as f64;
                assert_eq!(letter.origin, [want, 0.0], "{text:?} {k}");
            }
        }
        // Half and whole pair prefixes differ from the summed unit widths.
        let pair: Vec<u16> = "😀".encode_utf16().collect();
        assert_eq!(width(&pair[..1]), width(&[0xfffd]));
        let l = placed(json!({"text": "😀A"}));
        assert!(
            !l[0].paths.is_empty() && !l[1].paths.is_empty(),
            "two replacement outlines"
        );
        let (a, b) = (ink_box(&l[0]), ink_box(&l[1]));
        assert!(((a[2] - a[0]) - (b[2] - b[0])).abs() < 0.01);
        assert_eq!(l[2].counters.unwrap().index, 2);
        // Selectors and a zero-width space count but draw nothing.
        let s = placed(json!({"text": "A\u{fe0f}\u{200b}B"}));
        assert!(s[1].paths.is_empty() && s[2].paths.is_empty());
        assert_eq!(s[3].counters.unwrap().index, 3);
        // The size is the em in pixels: 114 is not 152.
        let one = placed(json!({"text": "█"}));
        assert!((ink_box(&one[0])[2] - ink_box(&one[0])[0] - 68.4).abs() < 0.01);
    }

    /// `line(units[..k]).width`: the prefix laid out whole, as one line.
    fn whole(units: &[u16], family: &str, bold: bool) -> f64 {
        layout_raw_units(&[], &[units.to_vec()], family, 114.0, bold, false).1[0].width
    }

    /// Bundled Noto Color Emoji under its own family, as the primary face (CBDT bitmaps).
    const NOTO_COLOR: &[u8] =
        include_bytes!("../../../../../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf");

    /// R158 Q68 in one pass: every raw-prefix width equals that prefix laid out whole, exactly,
    /// across pairs split or whole, VS15/VS16 after BMP, pair and lone units, colour bitmaps,
    /// combining marks, other whitespace and kerning-free runs.
    #[test]
    fn prefix_widths_equal_whole_prefixes() {
        use crate::fonts::{self, FaceStyle};
        fonts::register_lazy_face("bb31-colour", FaceStyle::Regular, NOTO_COLOR.to_vec()).unwrap();
        let texts = [
            "A B  C ",
            "😀A😀",
            "A😀B",
            "☺\u{fe0f}A☺\u{fe0e}B☺",
            "😀\u{fe0f}😀\u{fe0e}🎂",
            "a\u{301}b\u{308}\u{301}",
            "AV To WA",
            "x\n\r\t\u{a0}\u{200b}y",
            "\u{fe0f}A\u{fe0e}\u{fe0e}",
            "🎂🎂\u{fe0e}🎂\u{fe0f}A",
        ];
        let mut cases: Vec<Vec<u16>> = texts.iter().map(|t| t.encode_utf16().collect()).collect();
        // Lone halves: a high before a non-low, a low first, a high last, a high before VS16.
        cases.extend([
            vec![0xd83d, 0x41, 0xd83d],
            vec![0xde00, 0x41, 0xd83d, 0xde00],
            vec![0x41, 0xd83d, 0xfe0f, 0x42],
            vec![0xd83d, 0xd83d, 0xde00, 0xfe0f],
        ]);
        for family in [FONT, "bb31-colour", "Selawik"] {
            for bold in [false, true] {
                for units in &cases {
                    let got = layout_raw_units(units, &[], family, 114.0, bold, false).0;
                    assert_eq!(got.len(), units.len());
                    for (k, w) in got.iter().enumerate() {
                        assert_eq!(
                            *w,
                            whole(&units[..k], family, bold),
                            "{family} {units:x?} {k}"
                        );
                    }
                }
            }
        }
        // The colour face really draws bitmaps here, and the drawn unit never does.
        let run = layout_raw_units(
            &[],
            &[vec![0xd83c, 0xdf82]],
            "bb31-colour",
            40.0,
            false,
            false,
        );
        assert!(
            run.1[0]
                .items
                .iter()
                .any(|i| matches!(i, DrawItem::Bitmap(_)))
        );
        let (_, drawn) = raw_units(&[0x263a, 0x20], "bb31-colour", 40.0, false, false);
        assert!(drawn[1].is_none(), "no line for a space");
        let projected = drawn[0].as_ref().unwrap();
        assert!(
            projected
                .items
                .iter()
                .all(|i| matches!(i, DrawItem::Outline(_)))
        );
        fonts::unregister_family("bb31-colour");
    }

    /// The layout work is linear in the units: one line for all prefixes, one per drawn unit and a
    /// short one per split pair or selector, never each prefix whole (1024 units: ~524k scalars).
    #[test]
    fn layout_work_is_linear() {
        use crate::tool::text::LINE_GLYPHS;
        let count = |text: &str| {
            LINE_GLYPHS.with(|n| n.set(0));
            let l = placed(json!({"text": text, "size": 10}));
            assert_eq!(l.len(), text.encode_utf16().count());
            LINE_GLYPHS.with(|n| n.get())
        };
        let latin = "Fun Text ".repeat(114)[..1024].to_string();
        assert!(count(&latin) <= 4 * 1024, "{}", count(&latin));
        // 1024 units of pairs and selectors: every other prefix is special.
        let emoji = "😀\u{fe0f}A".repeat(256);
        assert_eq!(emoji.encode_utf16().count(), 1024);
        assert!(count(&emoji) <= 5 * 1024, "{}", count(&emoji));
    }

    /// R187 / R76: `TEXT_FUN_ROTATION_CLOCKWISE` decides the sense. Clockwise moves the top-right
    /// corner right and down; flipped, it moves left and up (the top-left corner left and down).
    #[test]
    fn rotation_sense_is_the_convention() {
        let (c, tl, tr) = ([0.0, 0.0], (-34.0, -69.0), (34.0, -69.0));
        // The adopted sense (R187) is clockwise.
        assert_eq!(
            rotation(30.0, TEXT_FUN_ROTATION_CLOCKWISE, c),
            rotation(30.0, true, c)
        );
        let map = |t: Transform, (x, y): (f32, f32)| {
            let mut p = [tiny_skia::Point::from_xy(x, y)];
            t.map_points(&mut p);
            (p[0].x, p[0].y)
        };
        let cw = rotation(30.0, true, c);
        let (r, l) = (map(cw, tr), map(cw, tl));
        assert!(
            r.0 > tr.0 && r.1 > tr.1 && l.0 > tl.0 && l.1 < tl.1,
            "{r:?} {l:?}"
        );
        let ccw = rotation(30.0, false, c);
        let (r, l) = (map(ccw, tr), map(ccw, tl));
        assert!(
            r.0 < tr.0 && r.1 < tr.1 && l.0 < tl.0 && l.1 > tl.1,
            "{r:?} {l:?}"
        );
        // The flipped sense is the clockwise turn by the opposite angle.
        assert_eq!(map(ccw, tr), map(rotation(-30.0, true, c), tr));
    }

    /// R158 Q69: Bold requests the family's 700 face (Selawik Bold's own bytes); a face above 600
    /// with no bold slot keeps its bytes and gets no thickening; Italics requests italic.
    #[test]
    fn bold_and_italic_faces() {
        use crate::fonts::{self, FaceStyle};
        let face = |family: &str, bold: bool, italic: bool| {
            let l = layout_raw_units(&[], &[vec![0x41]], family, 50.0, bold, italic).1;
            (l[0].glyphs[0].face.clone(), l[0].items.len())
        };
        assert!(face("Selawik", true, false).0.as_ref() == fonts::SELAWIK_BOLD);
        assert!(face("Selawik", false, false).0.as_ref() == fonts::SELAWIK_REGULAR);
        fonts::register_lazy_face(
            "bb31-heavy",
            FaceStyle::Regular,
            fonts::CARLITO_BOLD.to_vec(),
        )
        .unwrap();
        let (bytes, items) = face("bb31-heavy", true, false);
        assert!(
            bytes.as_ref() == fonts::CARLITO_BOLD && items == 1,
            "no synthesis above 600"
        );
        fonts::unregister_family("bb31-heavy");
        assert_eq!(
            face("Noto Sans Mono", true, false).1,
            2,
            "R111 thickening at 400"
        );
        assert_eq!(
            TEXT_FUN_BOLD_FACE_FALLBACK(true, true),
            FaceStyle::BoldItalic
        );
        let def = find("bbTextFunFactory").unwrap();
        let fonts_of = |extra: Value| (def.indirect.unwrap().fonts)(&values(extra));
        assert_eq!(
            fonts_of(json!({"font": "Lato", "bold": true})),
            [("Lato".to_string(), FaceStyle::Bold)]
        );
        assert_eq!(
            fonts_of(json!({"font": "Lato", "italics": true})),
            [("Lato".to_string(), FaceStyle::Italic)]
        );
        // A colour-emoji prefix needs the lazy colour face before it is measured.
        assert!(
            fonts_of(json!({"text": "a🎂"}))
                .contains(&("Noto Color Emoji".to_string(), FaceStyle::Regular))
        );
    }

    /// Q9: the default is the first "Rounded" family by name, else SANS_FONT; a missing saved
    /// family renders in it through the transient fallback, the token untouched.
    #[test]
    fn default_and_fallback_fonts() {
        assert_eq!(
            default_font([
                ("b", "Zed Rounded"),
                ("a", "Arial Rounded MT Bold"),
                ("c", "Arial")
            ]),
            "a"
        );
        assert_eq!(
            default_font([("x", "Arial")]),
            super::super::conventions::SANS_FONT
        );
        let def = find("bbTextFunFactory").unwrap();
        let ui = def.indirect.unwrap();
        let mut v = values(json!({"font": "bb31-never-installed"}));
        assert!((ui.font_fallback)(&mut v));
        assert_eq!(v["font"], json!("bb31-never-installed"));
        assert_eq!(
            (ui.fonts)(&v)[0].0,
            super::super::conventions::SANS_FONT,
            "the session renders the fallback"
        );
        assert!(!(ui.font_fallback)(&mut v), "only once");
        assert_eq!(ui.transient_keys, ["fallbackFont", "fallbackFamily"]);
        // A registered family never falls back (another face failed).
        let mut v = values(json!({"font": FONT}));
        assert!(!(ui.font_fallback)(&mut v));
        // TFF-sync review Minor 1 (Q69): the fallback is the Q9 default of the UI thread's
        // document list, picked there; a render worker (bundled list only) draws it too. That
        // default failing as well falls back to SANS_FONT, then no further.
        crate::fonts::set_document_choices(vec![
            ("bb-q9-rounded".into(), "Zed Rounded".into()),
            ("Arial".into(), "Arial".into()),
        ]);
        let mut v = values(json!({"font": "bb31-never-installed"}));
        assert!((ui.font_fallback)(&mut v));
        assert_eq!((ui.fonts)(&v)[0].0, "bb-q9-rounded");
        let shared = v.clone();
        let worker = std::thread::spawn(move || (ui.fonts)(&shared)[0].0.clone());
        assert_eq!(worker.join().unwrap(), "bb-q9-rounded");
        assert!(
            (ui.font_fallback)(&mut v),
            "the default is not installed either"
        );
        assert_eq!((ui.fonts)(&v)[0].0, super::super::conventions::SANS_FONT);
        assert!(!(ui.font_fallback)(&mut v));
        assert_eq!(v["font"], json!("bb31-never-installed"));
        // The missing family is the default itself: straight to SANS_FONT.
        let mut v = values(json!({"font": "bb-q9-rounded"}));
        assert!((ui.font_fallback)(&mut v));
        assert_eq!((ui.fonts)(&v)[0].0, super::super::conventions::SANS_FONT);
        crate::fonts::set_document_choices(vec![]);
        // R157: the defaults (static Selawik) are ready at once; a cold family is not.
        let d = super::super::indirect_defaults(&def, &EffectEnv::whole(W, H));
        assert!(super::super::text::faces_ready(&def, &d));
        assert!(!super::super::text::faces_ready(
            &def,
            &values(json!({"font": "bb31-cold"}))
        ));
    }

    /// common.md §2: the result is clipped to the selection exactly once, by the framework;
    /// outside it the source, hidden colour included, is kept.
    #[test]
    fn selection_clips_once() {
        let src = from_fn(W, H, |x, y| {
            [x as u8, y as u8, 9, if x % 7 == 0 { 0 } else { 200 }]
        });
        let (mask, polys) = ellipse_selection(W, H, [120.0, 100.0], [90.0, 70.0]);
        let env = with_selection(W, H, &mask, &polys);
        // R159: with the bevel and a metal face too.
        for extra in [
            json!({}),
            json!({"depth": 7}),
            json!({"fillTab": 2, "metal": "gold", "depth": 7}),
        ] {
            let v = base(merged(
                json!({"shadowLength": 12, "outlineSize": 3, "maintain": true}),
                &extra,
            ));
            let clipped = bb_env("bbTextFunFactory", v.clone(), &src, &env);
            let open = EffectEnv { mask: None, ..env };
            let mut want = bb_env("bbTextFunFactory", v, &src, &open);
            crate::effect::underwrite(&src, &mut want, &mask);
            assert_eq!(clipped.data, want.data);
            assert_eq!(px(&clipped, 0, 0), px(&src, 0, 0));
            assert_eq!(px(&clipped, 7, 290), px(&src, 7, 290));
        }
        // The text starts at the selection bounds' top-left.
        let b = env.bounds;
        let l = letters(&values(json!({})), &env);
        assert_eq!(l[0].origin, [b.x as f64 + 10.0, b.y as f64]);
    }

    /// Silver and Copper at 3D depth 0 are flat, exactly converted colours.
    #[test]
    fn flat_silver_and_copper() {
        for (metal, want) in [
            ("silver", [128, 128, 128, 255]),
            ("copper", [165, 25, 0, 255]),
        ] {
            assert_px(&run(json!({"fillTab": 2, "metal": metal})), 44, 80, want);
            assert_px(
                &run_alt(json!({"fillTab": 2, "metal": metal})),
                44,
                80,
                want,
            );
            assert_eq!(
                px(&run(json!({"fillTab": 2, "metal": metal})), 395, 295)[3],
                0
            );
        }
    }

    /// `a` with `b`'s keys set.
    fn merged(mut a: Value, b: &Value) -> Value {
        for (k, x) in b.as_object().unwrap() {
            a[k] = x.clone();
        }
        a
    }

    /// text-formations-text-fun-factory.md Expected results 9: at 3D Depth 7 every metal's
    /// interior (at least Depth + 3 px inside the block) is Silver (128, 128, 128), Copper
    /// (165, 25, 0) ±2, Gold (188, 171, 0) ±6 and Chrome ≤ 15 per channel, all opaque; the bevel
    /// band shows at the inner edges; everything outside the glyph beyond the length-0 shadow's
    /// antialiased fringe is transparent, Chrome included. In the encoded context Silver and
    /// Copper are the same converted colours and the raw gold flat is (128, 104, 0) (R159
    /// arithmetic, not a native observation).
    #[test]
    fn expected_9_metal_faces() {
        // The block covers [10, 78.4] × [10.944, 149.226] (block_geometry_is_the_fixture_face).
        let interior = [(21, 22), (44, 80), (67, 138), (21, 138), (67, 22)];
        for (metal, linear, alt, tol) in [
            ("silver", [128, 128, 128], [128, 128, 128], 2),
            ("copper", [165, 25, 0], [165, 25, 0], 2),
            ("gold", [188, 171, 0], [128, 104, 0], 6),
            ("chrome", [0, 0, 0], [0, 0, 0], 15),
        ] {
            let v = json!({"fillTab": 2, "metal": metal, "depth": 7, "light": -45,
                "metalStrength": 1, "metalHard": true, "overlayStyle": false});
            for (img, want) in [(run(v.clone()), linear), (run_alt(v), alt)] {
                for (x, y) in interior {
                    assert_px_tol(&img, x, y, [want[0], want[1], want[2], 255], tol);
                }
                let mid = px(&img, 44, 80);
                for (x, y) in [(12, 80), (76, 80), (44, 13), (44, 146)] {
                    let p = px(&img, x, y);
                    let d = (0..3).map(|c| p[c].abs_diff(mid[c])).max().unwrap();
                    assert!(d > 8, "{metal} band ({x}, {y}) {p:?} vs {mid:?}");
                    assert_eq!(p[3], 255);
                }
                for y in 0..H {
                    for x in 0..W {
                        if !(8..81).contains(&x) || !(9..152).contains(&y) {
                            assert_eq!(px(&img, x, y)[3], 0, "{metal} ({x}, {y})");
                        }
                    }
                }
            }
        }
    }

    /// text-formations-text-fun-factory.md "3D bevel" step 4 and "Each letter is bevelled on its
    /// own": at the default light (−45°) a letter's inner upper-left edges are lighter and its
    /// inner lower-right edges darker; 135° swaps them; overlapping letters keep their own edges
    /// (the first block's right edge shows inside the overlap); no lighting outside the letter.
    #[test]
    fn bevel_lights_upper_left_per_letter() {
        let grey = json!({"solidColor": "custom", "customColor": "#808080", "depth": 7,
            "bevelStrength": 0.75, "highlight": "#FFFFFF", "hardBevel": false,
            "overlayStyle": false});
        for (light, sign) in [(-45, 1), (135, -1)] {
            let img = run(merged(grey.clone(), &json!({ "light": light })));
            let mid = px(&img, 44, 80)[0] as i32;
            assert_eq!(mid, 128);
            for (x, y, s) in [(12, 80, 1), (44, 13, 1), (76, 80, -1), (44, 146, -1)] {
                let d = px(&img, x, y)[0] as i32 - mid;
                assert!(d * s * sign > 10, "light {light} ({x}, {y}): {d}");
            }
            assert_eq!(px(&img, 8, 80)[3], 0);
            assert_eq!(px(&img, 44, 9)[3], 0);
        }
        let two = run(merged(grey, &json!({"text": "██", "overlap": -12})));
        // The second block starts at 66.4; the first block is on top up to its edge at 78.4.
        assert!(px(&two, 76, 80)[0] + 10 < px(&two, 44, 80)[0]);
    }

    /// text-formations-text-fun-factory.md "Fill" Metallic and "Colour handling": Silver and Copper
    /// bevel with the Metallic Strength / Hard bevel and their raw highlights, the 3D tab's Overlay
    /// style and the shared depth; the 3D tab's highlight is raw too; Gold and Chrome always take
    /// their own pipeline, depth 0 included; other fills at depth 0 are flat.
    #[test]
    fn faces_route_metal_overrides() {
        let env = EffectEnv::whole(W, H);
        let contradictory = json!({"metalStrength": 0.4, "metalHard": false, "bevelStrength": 0.9,
            "hardBevel": true, "highlight": "#808080", "overlayStyle": true});
        let face = |extra: Value, depth| {
            Face::new(&values(merged(contradictory.clone(), &extra)), &env, depth)
        };
        let bevel = |strength, rgb: [u8; 3], hard| {
            Face::Bevel(Bevel {
                strength,
                highlight: colour_raw([rgb[0], rgb[1], rgb[2], 255]),
                hard,
                overlay: true,
            })
        };
        let metallic = |m| json!({"fillTab": 2, "metal": m});
        assert_eq!(face(metallic("silver"), 7.0), bevel(0.4, [200; 3], false));
        assert_eq!(
            face(metallic("copper"), 7.0),
            bevel(0.4, [163, 72, 58], false)
        );
        assert_eq!(face(json!({}), 7.0), bevel(0.9, [128; 3], true));
        assert_eq!(face(json!({"fillTab": 3}), 7.0), bevel(0.9, [128; 3], true));
        for f in [metallic("silver"), json!({"fillTab": 1})] {
            assert_eq!(face(f, 0.0), Face::Flat);
        }
        for depth in [0.0, 7.0] {
            assert_eq!(face(metallic("gold"), depth), Face::Metal(true));
            assert_eq!(face(metallic("chrome"), depth), Face::Metal(false));
        }
        // Gold / Chrome on another Fill sub-tab is not a metal.
        assert_eq!(
            face(merged(metallic("gold"), &json!({"fillTab": 0})), 7.0),
            bevel(0.9, [128; 3], true)
        );
    }

    /// text-formations-text-fun-factory.md "Fill" Metallic: Gold and Chrome ignore the 3D tab and
    /// the disabled metallic Strength / Hard bevel but follow the shared Depth and Light; Silver
    /// and Copper take Metallic Strength / Hard bevel and the metal's highlight in place of the
    /// 3D tab's, keep its Overlay style, and are flat at Strength 0.
    #[test]
    fn metal_controls_route() {
        for metal in ["gold", "chrome", "silver", "copper"] {
            let m = |extra: Value| {
                run(merged(
                    json!({"fillTab": 2, "metal": metal, "depth": 7}),
                    &extra,
                ))
                .data
            };
            let plain = m(json!({}));
            let ordinary =
                json!({"bevelStrength": 0.1, "highlight": "#000000", "hardBevel": false});
            if matches!(metal, "gold" | "chrome") {
                let ignored = merged(
                    ordinary,
                    &json!({"hardBevel": true, "overlayStyle": true, "metalStrength": 0,
                        "metalHard": false}),
                );
                assert!(plain == m(ignored), "{metal}");
                assert!(plain != m(json!({"depth": 3})), "{metal}");
                assert!(plain != m(json!({"light": 0})), "{metal}");
                // Chrome is bright on both slope signs: the opposite light gives the same face
                // (to a level); Gold's lit and shaded edges swap.
                let opposite = m(json!({"light": 135}));
                let same = plain
                    .iter()
                    .zip(&opposite)
                    .all(|(a, b)| a.abs_diff(*b) <= 1);
                assert_eq!(same, metal == "chrome");
            } else {
                assert!(plain == m(ordinary), "{metal}");
                assert!(plain != m(json!({"overlayStyle": true})), "{metal}");
                assert!(plain != m(json!({"metalHard": false})), "{metal}");
                let flat = run(merged(
                    json!({"fillTab": 2, "metal": metal, "depth": 7}),
                    &json!({"metalStrength": 0}),
                ));
                assert_eq!(px(&flat, 12, 80), px(&flat, 44, 80), "{metal}");
            }
        }
    }

    /// text-formations-text-fun-factory.md Expected results 10: the lighting map of "3D bevel"
    /// step 5 at Strength 0.75 with a white Highlight (r = 1 → highlight 0.75, 0.875 → 0.5625,
    /// 0.75 → 0.375, 0.5 → nothing, 0.25 → black 0.375, 0 → black 0.75; Strength 1 caps at 0.99);
    /// r = 1 over an opaque black fill gives 0.75 white in linear light, 225 encoded (191 in the
    /// encoded context); r = 0.5 leaves a fill unchanged.
    #[test]
    fn expected_10_bevel_lighting() {
        use super::super::blocks::{Shade, lighting};
        use crate::pixel::gamma::{linear_to_srgb8, quantize};
        for (r, want) in [
            (1.0, Some((Shade::Light, 0.75))),
            (0.875, Some((Shade::Light, 0.5625))),
            (0.75, Some((Shade::Light, 0.375))),
            (0.5, None),
            (0.25, Some((Shade::Dark, 0.375))),
            (0.0, Some((Shade::Dark, 0.75))),
        ] {
            let got = lighting(r, 0.75, DEAD_BAND);
            match (got, want) {
                (None, None) => {}
                (Some((s, o)), Some((ws, wo))) => {
                    assert!(s == ws && (o - wo).abs() < 1e-6, "{r}: {got:?}")
                }
                _ => panic!("{r}: {got:?}"),
            }
        }
        assert_eq!(lighting(1.0, 1.0, DEAD_BAND), Some((Shade::Light, 0.99)));
        assert_eq!(lighting(1.5, 0.75, DEAD_BAND), Some((Shade::Light, 0.99)));
        assert_eq!(lighting(-0.5, 0.75, DEAD_BAND), Some((Shade::Dark, 0.99)));
        assert_eq!(lighting(0.0, 0.0, DEAD_BAND), None);
        let white = colour_raw([255; 4]);
        let lit = light_fill([0.0, 0.0, 0.0, 1.0], 1.0, 0.75, white, false);
        assert!(lit[..3].iter().all(|&c| (c - 0.75).abs() < 1e-6) && lit[3] == 1.0);
        assert_eq!((linear_to_srgb8(lit[0]), quantize(lit[0])), (225, 191));
        let dark = light_fill([1.0; 4], 0.0, 0.75, white, false);
        assert!(dark[..3].iter().all(|&c| (c - 0.25).abs() < 1e-6) && dark[3] == 1.0);
        let fill = premultiply([0.3, 0.7, 0.55, 0.4]);
        for overlay in [false, true] {
            assert_eq!(light_fill(fill, 0.5, 0.75, white, overlay), fill);
            assert_eq!(light_fill(fill, 1.0, 0.0, white, overlay), fill);
        }
    }

    /// "3D bevel" steps 5–6: the dead band includes its 1 × 10⁻⁷ endpoint (checked on the f32
    /// neighbours of 0.5); normal source-over and the destination-decided Overlay on straight
    /// colour, then the fill's own alpha restored (R159 proposal §3 closed forms).
    #[test]
    fn bevel_lighting_band_and_styles() {
        use super::super::blocks::{Shade, lighting};
        let half = 0.5f32.to_bits();
        let at = |bits: u32| lighting(f32::from_bits(bits), 0.75, DEAD_BAND);
        // Above 0.5 the f32 step is 2⁻²⁴, below it 2⁻²⁵.
        assert_eq!(at(half + 1), None);
        assert!(matches!(at(half + 2), Some((Shade::Light, o)) if o > 0.0));
        assert_eq!(at(half - 3), None);
        assert!(matches!(at(half - 4), Some((Shade::Dark, o)) if o > 0.0));
        let r = f32::from_bits(half + 2);
        assert_eq!(lighting(r, 0.75, r - 0.5), None);
        let copper = colour_raw([163, 72, 58, 255]);
        for overlay in [false, true] {
            for a in [0.0f32, 0.25, 0.5, 1.0] {
                for b in [[0.3f32, 0.7, 0.5], [0.8, 0.2, 0.45]] {
                    for (r, s) in [(1.0f32, [copper[0], copper[1], copper[2]]), (0.1, [0.0; 3])] {
                        let (_, l) = lighting(r, 0.75, DEAD_BAND).unwrap();
                        let ao = l + a * (1.0 - l);
                        let want: [f32; 3] = std::array::from_fn(|c| {
                            let (b, s) = (b[c], s[c]);
                            let mixed = if b <= 0.5 {
                                2.0 * b * s
                            } else {
                                1.0 - 2.0 * (1.0 - b) * (1.0 - s)
                            };
                            let straight = if overlay {
                                s * l * (1.0 - a) + b * a * (1.0 - l) + a * l * mixed
                            } else {
                                s * l + b * a * (1.0 - l)
                            } / ao;
                            straight * a
                        });
                        let got = light_fill(
                            premultiply([b[0], b[1], b[2], a]),
                            r,
                            0.75,
                            copper,
                            overlay,
                        );
                        assert_eq!(got[3], a);
                        assert!(
                            (0..3).all(|c| (got[c] - want[c]).abs() < 1e-5),
                            "overlay {overlay} a {a} b {b:?} r {r}: {got:?} vs {want:?}"
                        );
                    }
                }
            }
        }
        // White lighting on opaque black: Overlay keeps it black, source-over brightens it.
        let white = colour_raw([255; 4]);
        assert_eq!(
            light_fill([0.0, 0.0, 0.0, 1.0], 1.0, 0.75, white, true),
            [0.0, 0.0, 0.0, 1.0]
        );
    }

    /// An `w × h` coverage of the rectangle [l, r] × [t, b]: each pixel's covered area.
    fn aa_rect(w: u32, h: u32, [l, t, r, b]: [f32; 4]) -> Vec<f32> {
        let span = |p: f32, lo: f32, hi: f32| (hi.min(p + 1.0) - lo.max(p)).max(0.0);
        (0..w * h)
            .map(|i| span((i % w) as f32, l, r) * span((i / w) as f32, t, b))
            .collect()
    }
    fn height_of(cover: &[f32], w: u32, f: impl Fn(f32) -> f32 + Sync) -> EffectSurface {
        EffectSurface::from_fn(w, cover.len() as u32 / w, |x, y| {
            let v = f(1.0 - cover[(y * w + x) as usize]);
            [v, v, v, 1.0]
        })
    }
    fn encoded(s: &EffectSurface) -> EffectSurface {
        EffectSurface::from_fn(s.w, s.h, |x, y| {
            let [r, g, b, a] = s.at(x, y);
            [
                linear_to_srgb_f32(r),
                linear_to_srgb_f32(g),
                linear_to_srgb_f32(b),
                a,
            ]
        })
    }

    /// "3D bevel" steps 1–3 (R159 Q72, Q73): the opaque height 1 − c softened by the shared HQ
    /// Gaussian at σ = D/3 (normalised once) or by plain Bokeh quality 4, gamma boost 0, Mirror,
    /// then its RGB sRGB-encoded before the shared quantise / Emboss tail. Not Bokeh 10, a boost,
    /// BB15's cube / cube-root, or the unencoded mask. A blank or fully covered buffer is flat.
    #[test]
    fn bevel_soften_and_encode_stages() {
        use super::super::blocks::{blur_soft, bokeh_with_edge};
        let (w, h) = (48, 40);
        let cover = aa_rect(w, h, [9.3, 8.7, 31.6, 25.2]);
        let plain = height_of(&cover, w, |v| v);
        let soft = bevel_height(&cover, w, 7.0, false);
        let gauss = blur_soft(&plain, 7.0 * RADIUS_TO_SIGMA);
        let gauss = EffectSurface::from_fn(w, h, |x, y| unpremultiply(gauss.at(x, y)));
        assert_eq!(soft.px, encoded(&gauss).px);
        assert_ne!(soft.px, gauss.px);
        let hard = bevel_height(&cover, w, 7.0, true);
        let bokeh = |s: &EffectSurface, q, boost| bokeh_with_edge(s, 7.0, q, boost, Edge::Mirror);
        assert_eq!(hard.px, encoded(&bokeh(&plain, 4, 0.0)).px);
        for other in [
            bokeh(&plain, 10, 0.0),
            bokeh(&plain, 4, 1.0),
            bokeh_with_edge(&plain, 7.0, 4, 0.0, Edge::Transparent),
            plain.clone(),
            EffectSurface::from_fn(w, h, |x, y| {
                let p = bokeh(&height_of(&cover, w, |v| v.powi(3)), 10, 0.0).at(x, y);
                [p[0].cbrt(), p[1].cbrt(), p[2].cbrt(), p[3]]
            }),
        ] {
            assert_ne!(hard.px, encoded(&other).px);
        }
        // The relief is the shared tail of the encoded stage, not of the linear one.
        let relief = super::super::bevel::relief_from_soft(&soft, -45.0);
        assert_ne!(
            relief.px,
            super::super::bevel::relief_from_soft(&gauss, -45.0).px
        );
        // Depth 0 adds nothing; a blank or a fully covered buffer leaves the fill as it is.
        let fill =
            EffectSurface::from_fn(w, h, |x, _| premultiply([0.2, 0.6, 0.9, x as f32 / 47.0]));
        let white = colour_raw([255; 4]);
        let bevel = |hard| Bevel {
            strength: 1.0,
            highlight: white,
            hard,
            overlay: false,
        };
        assert_eq!(
            bevel_letter(&fill, &cover, 0.0, -45.0, &bevel(false)).px,
            fill.px
        );
        for c in [0.0, 1.0] {
            for hard in [false, true] {
                let out = bevel_letter(&fill, &vec![c; cover.len()], 7.0, -45.0, &bevel(hard));
                assert_eq!(out.px, fill.px, "cover {c} hard {hard}");
            }
        }
    }

    /// "3D bevel" step 6: every bevelled pixel keeps the fill's own alpha exactly (a partly
    /// transparent Image fill keeps its alpha before the shadow), and alpha 0 stays empty.
    #[test]
    fn bevel_keeps_fill_alpha() {
        let (w, h) = (48, 40);
        let cover = aa_rect(w, h, [9.3, 8.7, 31.6, 25.2]);
        let fill = EffectSurface::from_fn(w, h, |x, y| {
            let c = cover[(y * w + x) as usize];
            let a = if x == 20 {
                0.0
            } else {
                c * (0.25 + 0.5 * (y % 2) as f32)
            };
            premultiply([0.9, 0.4, 0.1, a])
        });
        for (hard, overlay) in [(false, false), (true, true), (false, true)] {
            let b = Bevel {
                strength: 1.0,
                highlight: colour_raw([200, 200, 200, 255]),
                hard,
                overlay,
            };
            let out = bevel_letter(&fill, &cover, 7.0, -45.0, &b);
            assert_ne!(out.px, fill.px);
            for (o, f) in out.px.iter().zip(&fill.px) {
                assert_eq!(o[3], f[3]);
                if f[3] == 0.0 {
                    assert_eq!(*o, [0.0; 4]);
                }
            }
        }
    }

    /// "Gold and Chrome" steps 1–3 (R159 Q71): the 2.2 power acts on fractional coverage only
    /// (not its reciprocal, not skipped), then Bokeh quality 10 (none at depth 0) and the encode;
    /// step 6: the colour takes the letter's coverage as alpha exactly once, nothing outside it;
    /// depth 0 still embosses the edges.
    #[test]
    fn metal_power_relief_and_coverage() {
        use super::super::bevel::relief_from_soft;
        use super::super::blocks::bokeh_with_edge;
        let (w, h) = (48, 40);
        let cover = aa_rect(w, h, [9.3, 8.7, 31.6, 25.2]);
        let flat = metal_height(&[0.5, 0.0, 1.0], 3, 0.0);
        assert_eq!(flat.px[0][0], linear_to_srgb_f32(0.5f32.powf(2.2)));
        assert!((flat.px[0][0] - linear_to_srgb_f32(0.5f32.powf(1.0 / 2.2))).abs() > 0.3);
        assert!((flat.px[0][0] - linear_to_srgb_f32(0.5)).abs() > 0.2);
        // The binary endpoints are the plain encoded mask.
        assert_eq!(
            (flat.px[1][0], flat.px[2][0]),
            (linear_to_srgb_f32(1.0), 0.0)
        );
        let power = height_of(&cover, w, |v| v.powf(TEXT_FUN_METAL_MASK_POWER));
        let seven = metal_height(&cover, w, 7.0);
        let bokeh = |q| encoded(&bokeh_with_edge(&power, 7.0, q, 0.0, Edge::Mirror));
        assert_eq!(seven.px, bokeh(10).px);
        assert_ne!(seven.px, bokeh(4).px);
        for gold in [true, false] {
            for depth in [0.0, 7.0, 30.0] {
                let face = metal_face(&cover, w, depth, -45.0, gold);
                let relief = relief_from_soft(&metal_height(&cover, w, depth), -45.0);
                for (i, (p, &c)) in face.px.iter().zip(&cover).enumerate() {
                    assert_eq!(p[3], c);
                    let [r, g, b] = metal_colour(relief.px[i][0], gold);
                    assert_eq!(*p, premultiply([r, g, b, c]));
                }
            }
            // Depth 0: a flat interior, a lit or shaded edge.
            let face = metal_face(&cover, w, 0.0, -45.0, gold);
            let interior = face.at(20, 16);
            assert_eq!(
                interior,
                premultiply({
                    let [r, g, b] = metal_colour(0.5, gold);
                    [r, g, b, 1.0]
                })
            );
            assert_ne!(face.at(10, 16), interior);
            assert!(
                metal_face(&vec![0.0; cover.len()], w, 7.0, -45.0, gold)
                    .px
                    .iter()
                    .all(|p| *p == [0.0; 4])
            );
        }
    }

    /// "Gold and Chrome" step 5 (R159 Q74, relief unclamped): Gold is the Color Burn of raw gold
    /// onto E (red E, green 1 − (1 − E) ÷ (215/255) floored at 0, blue 0 except 1 at E = 1; E > 1
    /// keeps blue 0), Chrome the Difference of E and 1 − E (|2E − 1|).
    #[test]
    fn metal_colour_relief_range() {
        const { assert!(!TEXT_FUN_METAL_CLAMP_RELIEF) };
        let g = 215.0 / 255.0;
        for (e, gold, chrome) in [
            (-0.2f32, [0.0, 0.0, 0.0], 1.4),
            (0.0, [0.0, 0.0, 0.0], 1.0),
            (0.25, [0.25, 1.0 - 0.75 / g, 0.0], 0.5),
            (0.5, [0.5, 1.0 - 0.5 / g, 0.0], 0.0),
            (0.75, [0.75, 1.0 - 0.25 / g, 0.0], 0.5),
            (1.0, [1.0, 1.0, 1.0], 1.0),
            (1.2, [1.2, 1.0 + 0.2 / g, 0.0], 1.4),
        ] {
            let got = metal_colour(e, true);
            assert!(
                (0..3).all(|c| (got[c] - gold[c]).abs() < 1e-5),
                "gold {e}: {got:?}"
            );
            let got = metal_colour(e, false);
            assert!(
                got.iter().all(|&c| (c - chrome).abs() < 1e-5),
                "chrome {e}: {got:?}"
            );
        }
    }

    /// R159 halo: the bevel's blur support and the Emboss neighbour widen each letter's buffer, so
    /// a block edge just off the layer still bevels the pixels inside it. The same text on a
    /// larger canvas, through a selection, gives the same pixels.
    #[test]
    fn bevel_reaches_past_the_layer() {
        // The reference's margin (100) exceeds the largest reach here (depth 30: 60 + 1 + 2).
        let (w, h, m) = (120, 96, 100);
        let big = solid(w + 2 * m, h + 2 * m, [255; 4]);
        let (mask, polys) = rect_selection(w + 2 * m, h + 2 * m, m, m, w + m, h + m);
        let env = with_selection(w + 2 * m, h + 2 * m, &mask, &polys);
        let canvas = solid(w, h, [255; 4]);
        for extra in [
            // The left edge 4 px off the layer (soft), then the deepest soft bevel.
            json!({"depth": 7, "offsetX": -4}),
            json!({"depth": 30, "offsetX": -20}),
            // The right edge 4.4 px off (hard), the top 3.1 px off (Gold), the bottom 3.2 px off
            // (Chrome), Silver with a shadow cast inward, Gold's depth-0 Emboss.
            json!({"depth": 7, "hardBevel": true, "offsetX": 56}),
            json!({"fillTab": 2, "metal": "gold", "depth": 7, "offsetY": -14}),
            json!({"fillTab": 2, "metal": "chrome", "depth": 7, "offsetY": -50}),
            json!({"fillTab": 2, "metal": "silver", "depth": 7, "offsetX": 56,
                "shadowLength": 20, "light": 180}),
            json!({"fillTab": 2, "metal": "gold", "depth": 0, "offsetX": -1}),
        ] {
            let v = base(extra.clone());
            let small = bb("bbTextFunFactory", v.clone(), &canvas);
            let reference = bb_env("bbTextFunFactory", v, &big, &env);
            for y in 0..h {
                for x in 0..w {
                    assert_px(&small, x, y, px(&reference, x + m, y + m));
                }
            }
        }
        // The first case really bevels the layer's left column.
        let edge = bb(
            "bbTextFunFactory",
            base(json!({"depth": 7, "offsetX": -4})),
            &canvas,
        );
        assert!(px(&edge, 0, 60)[1] > px(&edge, 30, 60)[1] + 10);
    }

    /// The registered dialog: title, width, help, tabs, verbatim labels and captions, defaults,
    /// header rules and enable rules (text-formations-text-fun-factory.md "Controls").
    #[test]
    fn controls_and_presentation() {
        use crate::effect::Control;
        let def = find("bbTextFunFactory").unwrap();
        let ui = def.indirect.unwrap();
        assert_eq!(def.dialog_title, Some("Text Fun Factory"));
        assert_eq!(ui.width, 1.1);
        assert!(matches!(
            ui.help,
            super::super::ui::Help::Text(["Text Fun Factory v2.2"])
        ));
        let captions = |k: &str| {
            ui.tabs
                .iter()
                .find(|s| s.key == k)
                .unwrap()
                .tabs
                .iter()
                .map(|t| t.caption)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            captions("tab"),
            ["Text", "Fill", "Outline", "Shadow", "3D", "Fun"]
        );
        assert_eq!(
            captions("fillTab"),
            ["Solid", "Gradient", "Metallic", "Image"]
        );
        let page = |k: &str, i: usize| ui.tabs.iter().find(|s| s.key == k).unwrap().tabs[i].keys;
        assert_eq!(page("tab", 1), ["fillTab"]);
        assert!(page("fillTab", 2).contains(&"depth") && page("tab", 4).contains(&"depth"));
        assert!(page("fillTab", 2).contains(&"light") && page("tab", 3).contains(&"light"));
        let d = super::super::indirect_defaults(&def, &EffectEnv::whole(W, H));
        assert_eq!(
            (d["tab"].clone(), d["fillTab"].clone()),
            (json!(0), json!(0))
        );
        assert_eq!(d["text"], json!("Enter Text Here"));
        assert_eq!(d["font"], json!(super::super::conventions::SANS_FONT));
        for (k, want) in [
            ("size", json!(114.0)),
            ("offsetX", json!(10.0)),
            ("overlap", json!(-12.0)),
            ("reverseN", json!(2.0)),
            ("depth", json!(7.0)),
            ("light", json!(-45.0)),
            ("textureAngle", json!(90.0)),
            ("textureSat", json!(100.0)),
            ("shadowStrength", json!(0.75)),
            ("metalStrength", json!(1.0)),
            ("outlineSize", json!(3.0)),
            ("shadowLength", json!(5.0)),
            ("corner", json!("round")),
            ("texture", json!("t0")),
            ("solidColor", json!("red")),
            ("gradTo", json!("#0000ff")),
            ("srgbGradient", json!(true)),
            ("metalHard", json!(true)),
            ("maintain", json!(false)),
        ] {
            assert_eq!(d[k], want, "{k}");
        }
        let control = |k: &str| def.controls.iter().find(|c| c.key() == k).unwrap();
        let label = |k: &str| match control(k) {
            Control::Range { label, .. }
            | Control::Checkbox { label, .. }
            | Control::Select { label, .. }
            | Control::Color { label, .. }
            | Control::Text { label, .. }
            | Control::FontSelect { label, .. } => *label,
            c => panic!("{c:?}"),
        };
        let jumble = format!("\nEvens{}Jumble:{}Odds", " ".repeat(22), " ".repeat(21));
        let overlap = format!(
            "\nOverlap{}Kerning:{}Spread",
            " ".repeat(18),
            " ".repeat(18)
        );
        assert_eq!(jumble.chars().count(), 60);
        assert_eq!(label("jumble"), jumble);
        assert_eq!(label("overlap"), overlap);
        assert_eq!(
            label("offsetX"),
            "\nFine tune text placement (Left-Right / Up-Down)"
        );
        assert_eq!(label("customColor"), "\nCustom Color");
        assert_eq!(label("outlineColor"), "\nColor");
        assert_eq!(label("solidColor"), " ");
        assert_eq!(label("outlineOnTop"), "Outline on top of fill");
        assert_eq!(label("swapLayering"), "Swap layering direction");
        assert!(matches!(
            control("text"),
            Control::Text {
                label: "",
                default: "Enter Text Here",
                max_len: 1024,
                multiline: false,
                ..
            }
        ));
        for (k, caption) in [
            ("font", "Font: "),
            ("solidColor", "Color: "),
            ("gradStyle", "Style: "),
            ("metal", "Metal: "),
            ("texture", "Texture: "),
            ("corner", "Outline corner style: "),
            ("rotate", "Rotate: "),
            ("reverse", "Яeverse: "),
        ] {
            assert_eq!(ui.control(k).caption, caption, "{k}");
        }
        // Header rules only for Яeverse and the Saturation slider, both unlabelled.
        for c in def
            .controls
            .iter()
            .filter(|c| !matches!(c, Control::Value { .. }))
        {
            let k = c.key();
            assert_eq!(
                ui.control(k).header_line,
                matches!(k, "reverse" | "textureSat"),
                "{k}"
            );
        }
        assert_eq!(ui.control("outlineOnTop").header, Some(" "));
        assert!(
            ui.controls
                .iter()
                .filter(|(_, c)| c.header.is_some())
                .count()
                == 1
        );
        assert!(!ui.control("gradFrom").reset && !ui.control("shadowColor").reset);
        assert!(ui.control("customColor").reset && ui.control("outlineColor").reset);
        assert!(matches!(
            ui.control("customColor").note,
            super::super::ui::Note::Text(
                "\nThe solid color will be used if Fill > Image > Clipboard is\nchosen and the clipboard does not contain an image."
            )
        ));
        assert!(matches!(
            ui.control("highlight").note,
            super::super::ui::Note::Text(
                "3D light direction is the same as on the Shadow tab.\n3D for Metal fills are configured on Fill > Metallic tab."
            )
        ));
        let Control::Select { choices, .. } = control("texture") else {
            panic!()
        };
        assert_eq!(choices.len(), 50);
        assert_eq!(
            (choices[0], choices[49]),
            (("t0", "Copper Pipe"), ("t49", "Use Image on Clipboard"))
        );
        let Control::Select { choices, .. } = control("reverse") else {
            panic!()
        };
        assert_eq!(choices[1].1, "every number of letters:");
        // Enable rules; disabled values persist.
        let on = |extra: Value, k: &str| (ui.enabled)(&values(extra), k);
        assert!(
            !on(json!({}), "customColor") && on(json!({"solidColor": "custom"}), "customColor")
        );
        for k in ["outlineColor", "corner", "outlineOnTop"] {
            assert!(on(json!({"outlineSize": 1}), k) && !on(json!({"outlineSize": 0}), k));
        }
        for k in ["shadowColor", "shadowStrength"] {
            assert!(on(json!({"shadowLength": 1}), k) && !on(json!({"shadowLength": 0}), k));
        }
        for k in ["bevelStrength", "highlight", "hardBevel", "overlayStyle"] {
            assert!(on(json!({"depth": 1}), k) && !on(json!({"depth": 0}), k));
        }
        assert!(on(json!({"depth": 0}), "depth") && on(json!({"shadowLength": 0}), "light"));
        for m in ["gold", "chrome"] {
            assert!(
                !on(json!({"metal": m}), "metalStrength") && !on(json!({"metal": m}), "metalHard")
            );
        }
        assert!(on(json!({"metal": "copper"}), "metalStrength"));
        assert!(!on(json!({}), "rotateAngle") && on(json!({"rotate": "same"}), "rotateAngle"));
        for (r, want) in [
            ("none", false),
            ("every", true),
            ("only", true),
            ("firstOfWord", false),
        ] {
            assert_eq!(on(json!({"reverse": r}), "reverseN"), want);
        }
        // Slider metadata: integer, decimal and angle rows.
        let keys = |k: &str| match control(k) {
            Control::Range {
                step,
                keyboard,
                exponential,
                ..
            } => (*step, *keyboard, *exponential),
            c => panic!("{c:?}"),
        };
        let (step, k, exp) = keys("scale");
        assert!(
            step == 0.01
                && k.decimals == 3
                && k.small == 0.05
                && k.large == 0.25
                && k.exponential
                && exp
        );
        for s in ["metalStrength", "shadowStrength", "bevelStrength"] {
            let (step, k, exp) = keys(s);
            assert!(
                step == 0.01 && k.decimals == 3 && k.small == 0.05 && !k.exponential && !exp,
                "{s}"
            );
            assert_eq!(ui.control(s).decimals, Some(3));
        }
        for a in ["light", "textureAngle"] {
            let (step, k, _) = keys(a);
            assert!(
                step == 1.0 && k.decimals == 3 && k.small == 1.0 && k.large == 5.0,
                "{a}"
            );
            assert!(ui.control(a).wrap && ui.control(a).decimals == Some(3));
        }
        for i in [
            "size",
            "rotateAngle",
            "jumble",
            "overlap",
            "reverseN",
            "textureHue",
            "textureSat",
        ] {
            let (step, k, _) = keys(i);
            let int = (k.decimals, k.small, k.large, k.exponential) == (0, 1.0, 5.0, false);
            assert!(step == 1.0 && int, "{i}");
            assert!(!ui.control(i).wrap, "{i}");
        }
    }
}
