//! BB spec: common.md §5.10
//!
//! The plugins' document text through the Text tool's own layout (R152): bundled faces, Smooth
//! antialiased unhinted outlines, Text's fallback, line metrics and its no-kerning advances, laid
//! out from a top-left origin. Fixed 96 DPI (R47), so 1 pt = 4/3 px.
use crate::fonts::FontPolicy;
use crate::tool::text::{
    DrawItem, FontSizeMetric, TextAlign, TextLayout, TextLine, TextRenderingMode, TextSettings,
    layout_raw_units_px, layout_text_px,
};
use tiny_skia::{FillRule, LineJoin, Mask, Path, Stroke, Transform};

/// common.md §5.10: points to pixels at the fixed 96 DPI.
pub fn pt_to_px(pt: f32) -> f32 {
    pt * 96.0 / 72.0
}

/// A document-text run laid out from a top-left origin (R152).
#[derive(Clone, Debug)]
pub struct GlyphRun {
    /// Text's ordered items, outlines and colour bitmaps alike, line-relative with the baseline
    /// already applied; each line's origin is the run origin plus the preceding line heights.
    pub layout: TextLayout,
    /// The widest line's advance, trailing spaces included (0 when empty).
    pub width: f64,
    /// The sum of the line heights.
    pub height: f64,
    /// The first line's baseline below its top.
    pub ascent: f64,
}

impl GlyphRun {
    /// The outline items in canvas space, in layout order. Bitmap items stay in `layout`.
    pub fn outlines(&self) -> Vec<Path> {
        self.layout
            .lines
            .iter()
            .flat_map(|l| {
                let at = Transform::from_translate(l.origin[0] as f32, l.origin[1] as f32);
                l.items.iter().filter_map(move |i| match i {
                    DrawItem::Outline(p) => p.clone().transform(at),
                    DrawItem::Bitmap(_) => None,
                })
            })
            .collect()
    }
}

/// The document-text settings `glyph_run` lays out with: Smooth, antialiased, left-aligned,
/// Bundled interface faces (R47).
pub fn settings(family: &str, bold: bool, italic: bool) -> TextSettings {
    TextSettings {
        family: family.into(),
        size: 0.0,
        metric: FontSizeMetric::Fixed96Dpi,
        bold,
        italic,
        underline: false,
        strikeout: false,
        mode: TextRenderingMode::Smooth,
        align: TextAlign::Left,
        antialias: true,
        policy: FontPolicy::Bundled,
    }
}

/// `text` in `family` at an em of `px`, its layout box's top-left at `origin` and the first
/// baseline one ascent below it. Lines split at '\n'.
pub fn glyph_run(text: &str, family: &str, px: f32, bold: bool, origin: [f64; 2]) -> GlyphRun {
    let s = settings(family, bold, false);
    let mut layout = layout_text_px(text, &s, [0.0, 0.0], px as f64);
    let mut y = origin[1];
    for l in &mut layout.lines {
        // Text's own origins floor the click and round the line: replace, not adjust, them.
        l.origin = [origin[0], y];
        y += l.height;
    }
    let ascent = |layout: &TextLayout| layout.lines[0].glyphs.first().map(|g| g.origin[1]);
    let ascent = ascent(&layout)
        .unwrap_or_else(|| ascent(&layout_text_px(" ", &s, [0.0, 0.0], px as f64)).unwrap_or(0.0));
    GlyphRun {
        width: layout.lines.iter().map(|l| l.width).fold(0.0, f64::max),
        height: y - origin[1],
        ascent,
        layout,
    }
}

/// R158 Q68 in `family` at an em of `px` (Text's settings as `glyph_run`, plus italic), faces
/// resolved once: the measured width of each raw prefix `units[..k]`, and each of `lines` as one
/// line (origin (0, 0), items on the baseline one ascent down; no LF split, CR, LF and lone
/// surrogates stay units).
pub fn layout_raw_units(
    units: &[u16],
    lines: &[Vec<u16>],
    family: &str,
    px: f32,
    bold: bool,
    italic: bool,
) -> (Vec<f64>, Vec<TextLine>) {
    layout_raw_units_px(units, lines, &settings(family, bold, italic), px as f64)
}

/// Each path's winding coverage, accumulated as Text fills its items: filled, or stroked `width`
/// wide with round joins. Row-major over a w×h canvas, 0…1.
pub fn coverage(paths: &[Path], stroke: Option<f32>, w: u32, h: u32) -> Vec<f32> {
    let Some(mut mask) = Mask::new(w, h) else {
        return vec![0.0; w as usize * h as usize];
    };
    for p in paths {
        let stroked;
        let p = match stroke {
            None => p,
            Some(width) => {
                let pen = Stroke {
                    width,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                };
                let Some(outline) = p.stroke(&pen, 1.0) else {
                    continue;
                };
                stroked = outline;
                &stroked
            }
        };
        mask.fill_path(p, FillRule::Winding, true, Transform::identity());
    }
    mask.data().iter().map(|&b| b as f32 / 255.0).collect()
}

/// The outline-presentation projection of a label drawn as outlines
/// (`conventions::DIMENSIONS_OUTLINE_PRESENTATION`). Every scalar but a line feed requests text
/// presentation (U+FE0E), replacing an explicit selector, so Text picks outline faces, or its
/// visible missing-glyph outline. The stored string is never changed.
pub fn outline_presentation(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 4);
    for c in text
        .chars()
        .filter(|c| !matches!(c, '\u{fe0e}' | '\u{fe0f}'))
    {
        out.push(c);
        if c != '\n' {
            out.push('\u{fe0e}');
        }
    }
    out
}

/// R157: every document face `def` draws `values` with is registered (static bundled faces
/// always are), so a synchronous render cannot substitute a cold face.
pub fn faces_ready(def: &crate::effect::EffectDef, values: &crate::effect::Values) -> bool {
    def.indirect.is_none_or(|u| {
        (u.fonts)(values)
            .iter()
            .all(|(family, style)| crate::fonts::registered_face(family, *style).is_some())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::bb::conventions::{HORIZON_INDICATOR_FONT, SANS_FONT};
    use crate::fonts::{self, FaceStyle};

    #[test]
    fn points_are_four_thirds_pixels() {
        assert_eq!((pt_to_px(12.0), pt_to_px(3.0)), (16.0, 4.0));
        assert!((pt_to_px(20.0) - 26.666666).abs() < 1e-5);
    }

    /// R152: the top-left adapter replaces Text's click-derived origins (no flooring, no
    /// rounding, no second ascent), keeps trailing spaces and Text's actual line metrics.
    #[test]
    fn glyph_runs_lay_out_from_a_top_left_origin() {
        let px = 16.0;
        let run = glyph_run("Ag\nB", SANS_FONT, px, false, [10.25, 20.75]);
        let (a, b) = (&run.layout.lines[0], &run.layout.lines[1]);
        assert_eq!(
            (a.origin, b.origin),
            ([10.25, 20.75], [10.25, 20.75 + a.height])
        );
        assert_eq!(run.height, a.height + b.height);
        assert_eq!(run.width, a.width.max(b.width));
        // Selawik's Windows metrics at 16 px (fonts::win_metrics): ascent 2210, height 2724.
        let em = |u: f64| u * px as f64 / 2048.0;
        assert!((run.ascent - em(2210.0)).abs() < 1e-6 && (a.height - em(2724.0)).abs() < 1e-5);
        // Outline items already carry the baseline: the "A" sits on it, one ascent down.
        let ink = run.outlines()[0].bounds();
        assert!(
            (ink.bottom() as f64 - (20.75 + run.ascent)).abs() < 0.05,
            "{ink:?}"
        );
        assert!(ink.left() as f64 >= 10.25 && (ink.top() as f64) > 20.75);
        let trailing = glyph_run("ab  ", SANS_FONT, px, false, [0.0; 2]).width;
        assert!(trailing > glyph_run("ab", SANS_FONT, px, false, [0.0; 2]).width);
        // Empty and whitespace-only runs keep a line box but no ink.
        let empty = glyph_run("", SANS_FONT, px, false, [3.0, 4.0]);
        assert_eq!((empty.width, empty.ascent), (0.0, run.ascent));
        assert!((empty.height - a.height).abs() < 1e-9 && empty.outlines().is_empty());
        let space = glyph_run("   ", SANS_FONT, px, false, [0.0; 2]);
        assert!(space.width > 0.0 && space.outlines().is_empty());
        // Bold resolves Selawik's real bold face.
        let bold = glyph_run("A", SANS_FONT, px, true, [0.0; 2]);
        assert!(bold.layout.lines[0].glyphs[0].face.as_ref() == fonts::SELAWIK_BOLD);
    }

    /// R152 Q51: Selawik has no ↔/↕; the bundled Carlito has both, and the degree sign.
    #[test]
    fn indicator_face_has_both_arrows() {
        for arrow in ["↔", "↕"] {
            let carlito = glyph_run(arrow, HORIZON_INDICATOR_FONT, 20.0, false, [0.0; 2]);
            let g = &carlito.layout.lines[0].glyphs[0];
            assert!(
                g.glyph_id != 0 && g.face.as_ref() == fonts::CARLITO_REGULAR,
                "{arrow}"
            );
            assert!(!carlito.outlines().is_empty() && carlito.width > 0.0);
            let selawik = glyph_run(arrow, SANS_FONT, 20.0, false, [0.0; 2]);
            assert_eq!(
                selawik.layout.lines[0].glyphs[0].glyph_id, 0,
                "Selawik lacks {arrow}"
            );
        }
        let degree = glyph_run("°", SANS_FONT, 20.0, false, [0.0; 2]);
        assert!(degree.layout.lines[0].glyphs[0].face.as_ref() == fonts::SELAWIK_REGULAR);
        assert_ne!(degree.layout.lines[0].glyphs[0].glyph_id, 0);
    }

    /// R152: the generic run keeps colour bitmaps in order; the Q52 projection turns them into
    /// outlines (the visible missing-glyph outline) without touching the input.
    #[test]
    fn colour_items_stay_and_the_projection_draws_outlines() {
        const NOTO: &[u8] =
            include_bytes!("../../../../../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf");
        fonts::register_lazy_face("bb28-colour-run", FaceStyle::Regular, NOTO.to_vec()).unwrap();
        let run = glyph_run("🎂", "bb28-colour-run", 20.0, false, [0.0; 2]);
        let items = &run.layout.lines[0].items;
        assert!(matches!(items[..], [DrawItem::Bitmap(_)]) && run.outlines().is_empty());
        let text = "a🎂\u{fe0f}\nb";
        let projected = outline_presentation(text);
        assert_eq!(projected, "a\u{fe0e}🎂\u{fe0e}\nb\u{fe0e}");
        let run = glyph_run(&projected, "bb28-colour-run", 20.0, false, [0.0; 2]);
        assert_eq!(run.layout.lines.len(), 2);
        assert!(
            run.layout
                .lines
                .iter()
                .flat_map(|l| &l.items)
                .all(|i| matches!(i, DrawItem::Outline(_)))
        );
        assert!(!run.outlines().is_empty());
        assert_eq!(text, "a🎂\u{fe0f}\nb");
        fonts::unregister_family("bb28-colour-run");
    }

    /// Winding fills accumulate per item; strokes widen each outline with round joins.
    #[test]
    fn coverage_fills_and_strokes() {
        let square = |x: f32| {
            tiny_skia::PathBuilder::from_rect(tiny_skia::Rect::from_xywh(x, 2.0, 4.0, 4.0).unwrap())
        };
        // Two overlapping opposite-direction contours in separate items still both cover.
        let reversed = square(6.0)
            .transform(Transform::from_row(-1.0, 0.0, 0.0, 1.0, 14.0, 0.0))
            .unwrap();
        let c = coverage(&[square(2.0), reversed], None, 12, 8);
        assert_eq!(
            (c[3 * 12 + 2], c[3 * 12 + 5], c[3 * 12 + 7], c[3 * 12 + 9]),
            (1.0, 1.0, 1.0, 0.0)
        );
        let s = coverage(&[square(4.0)], Some(2.0), 12, 8);
        assert_eq!(
            (s[3 * 12 + 3], s[3 * 12 + 6], s[3 * 12 + 2]),
            (1.0, 0.0, 0.0)
        );
    }

    /// R157: static bundled faces are ready at once; an unregistered family is not.
    #[test]
    fn faces_ready_checks_every_required_face() {
        use crate::effect::bb::ui::IndirectUi;
        static STATIC: IndirectUi = IndirectUi {
            fonts: |_| {
                vec![
                    (SANS_FONT.into(), FaceStyle::Regular),
                    (HORIZON_INDICATOR_FONT.into(), FaceStyle::Regular),
                ]
            },
            ..IndirectUi::DEFAULT
        };
        static COLD: IndirectUi = IndirectUi {
            fonts: |v| {
                vec![
                    (SANS_FONT.into(), FaceStyle::Regular),
                    (
                        crate::effect::choice(v, "family").into(),
                        FaceStyle::Regular,
                    ),
                ]
            },
            ..IndirectUi::DEFAULT
        };
        let def = |ui| crate::effect::EffectDef {
            indirect: Some(ui),
            ..crate::effect::find("invertColorsEffect").unwrap()
        };
        let v: crate::effect::Values =
            serde_json::from_value(serde_json::json!({"family": "bb28-cold-face"})).unwrap();
        assert!(faces_ready(&def(&STATIC), &v));
        assert!(!faces_ready(&def(&COLD), &v));
        assert!(faces_ready(
            &crate::effect::find("invertColorsEffect").unwrap(),
            &v
        ));
        for id in ["bbComplementaryColors", "bbLevelHorizon", "bbDimensions"] {
            let d = crate::effect::find(id).unwrap();
            assert!(faces_ready(&d, &crate::effect::defaults(&d)), "{id}");
        }
    }
}
