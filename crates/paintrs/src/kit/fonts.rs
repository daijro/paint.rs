//! The text tool's bundled font faces in the UI (the font picker's previews).
//! Port of: js/src/engine/util/FontManager.js (`bundledFonts`), css/theme.css (`@font-face`)
//! Reference: paintdotjs @ b4e6fc5
use egui::{Color32, FontData, FontDefinitions, FontFamily, Painter, Rect};
use std::sync::Arc;

use super::text::{self, FontSpec};

/// css/theme.css `@font-face` sources by family (the WOFF2 one as its TTF).
use pdn_core::fonts::BUNDLED as FACES;

/// A family's bundled face: the CSS generics (the picker's "System ..." entries) resolve as
/// `pdn_core::fonts::resolve_family` says (R10: no system fonts).
fn face_of(family: &str) -> Option<&'static str> {
    let family = pdn_core::fonts::resolve_family(family);
    FACES.iter().find(|(f, _)| *f == family).map(|(f, _)| *f)
}

/// A bundled family's face file (CSS generics resolved), when the build embeds it.
pub fn face_bytes(family: &str) -> Option<&'static [u8]> {
    let family = face_of(family)?;
    let (_, path) = FACES.iter().find(|(f, _)| *f == family)?;
    crate::assets::embedded_font(path)
}

/// `family`'s face at `path` for the UI: the embedded copy, else (R208: the web build embeds
/// none) the copy the font manager fetched once a preview asked for it, its instructions
/// emptied as build.rs empties the embedded ones.
fn ui_face(family: &str, path: &str) -> Option<FontData> {
    crate::assets::embedded_font(path)
        .map(FontData::from_static)
        .or_else(|| {
            let bytes =
                pdn_core::fonts::registered_face(family, pdn_core::fonts::FaceStyle::Regular)?;
            Some(FontData::from_owned(super::unhinted::without_instructions(
                &bytes,
            )))
        })
}

/// The UI faces' rasterization. Faces with TrueType instructions (Segoe UI) take Paint.NET's
/// model (`ui-text-and-glyphs.md §2.4` steps 3 and 6: interpreter hinting per `gasp`, symmetric,
/// natural/linear layout). Instruction-less faces (Selawik) are light-autohinted: Paint.NET
/// leaves them unhinted, but FreeType's `hintslight` autohinting matched the measured text better
/// (unhinted Selawik was off in 27 text rects, task 17b).
pub fn pdn_hinting(bytes: &[u8]) -> egui::FontTweak {
    let instructed = pdn_core::fonts::has_instructions(bytes);
    egui::FontTweak {
        hinting: Some(true),
        hinting_target: egui::epaint::text::HintingTarget::Smooth(
            egui::epaint::text::SmoothHinting {
                light: !instructed,
                symmetric_rendering: true,
                preserve_linear_metrics: true,
            },
        ),
        ..Default::default()
    }
}

/// The About dialog's wordmark face: Inter at `font-weight: 300` with the optical size Chromium
/// picks at 50 px (`font-optical-sizing: auto`, Inter's `opsz` axis tops out at 32).
pub const INTER_LIGHT: &str = "Inter Light";

/// Adds every bundled face as `FontFamily::Name(family)`, Noto Sans as its fallback. Noto Sans
/// and Noto Sans Mono share the UI's font data; a face missing from the build (the web build
/// embeds none) renders in Noto Sans until the font manager has fetched it ([`ui_face`]).
/// Variable faces render at `wght` 400 (`font-weight: normal`) and the preview's `opsz`
/// (`.tool-font-dropdown-preview { font-size: 17px }` with `font-optical-sizing: auto`); every
/// face is light-autohinted like FreeType hints for Chromium (`unhinted::without_instructions`).
pub fn install(defs: &mut FontDefinitions) {
    let preview_size = crate::theme::TOOL_FONT_COLUMNS.2;
    for (family, path) in FACES {
        let data = match family {
            // ponytail: these two keep the UI's interpreter hinting (one copy of the bytes);
            // Chromium light-autohints them, within the text tolerance in the font picker.
            "Noto Sans" => "noto-sans".to_string(),
            "Noto Sans Mono" => "noto-sans-mono".to_string(),
            _ => match ui_face(family, path) {
                Some(data) => {
                    let tweak = egui::FontTweak {
                        coords: egui::epaint::text::VariationCoords::new([
                            (b"wght", 400.0),
                            (b"opsz", preview_size),
                        ]),
                        hinting: Some(true),
                        hinting_target: egui::epaint::text::HintingTarget::Smooth(
                            egui::epaint::text::SmoothHinting {
                                light: true,
                                symmetric_rendering: true,
                                preserve_linear_metrics: true,
                            },
                        ),
                        ..Default::default()
                    };
                    let name = format!("face:{family}");
                    defs.font_data
                        .insert(name.clone(), Arc::new(data.tweak(tweak)));
                    name
                }
                None => "noto-sans".to_string(),
            },
        };
        defs.families.insert(
            FontFamily::Name(family.into()),
            vec![data, "noto-sans".into()],
        );
    }
    let light = match ui_face("Inter", "inter/Inter-Variable.ttf") {
        Some(data) => {
            let tweak = egui::FontTweak {
                coords: egui::epaint::text::VariationCoords::new([
                    (b"wght", 300.0),
                    (b"opsz", 32.0),
                ]),
                ..pdn_hinting(&[])
            };
            defs.font_data
                .insert(INTER_LIGHT.into(), Arc::new(data.tweak(tweak)));
            INTER_LIGHT.to_string()
        }
        None => "noto-sans".to_string(),
    };
    defs.families.insert(
        FontFamily::Name(INTER_LIGHT.into()),
        vec![light, "noto-sans".into()],
    );
}

/// Blink's ascent/descent for a face (SkFontMetrics through FreeType: OS/2 typo metrics when
/// `USE_TYPO_METRICS` is set, else hhea), in em.
fn metrics(family: &str, path: &str) -> (f32, f32) {
    let fetched = pdn_core::fonts::registered_face(family, pdn_core::fonts::FaceStyle::Regular);
    let face = crate::assets::embedded_font(path)
        .or(fetched.as_deref())
        .and_then(|b| ttf_parser::Face::parse(b, 0).ok());
    let Some(face) = face else {
        return (1.069, 0.293);
    };
    let upem = face.units_per_em() as f32;
    // ttf-parser picks typo metrics under USE_TYPO_METRICS, like FreeType.
    (
        face.ascender() as f32 / upem,
        -(face.descender() as f32) / upem,
    )
}

/// A `size`-px `FontSpec` in a bundled `family` (CSS generics resolved) with its face's Blink
/// ascent and descent; the UI font when the build bundles no such face.
pub fn face_spec(family: &str, size: f32) -> FontSpec {
    let spec = FontSpec::new(size);
    let Some(face) = face_of(family) else {
        return spec;
    };
    let path = FACES
        .iter()
        .find(|(f, _)| *f == face)
        .map(|(_, p)| *p)
        .unwrap_or("");
    let (a, d) = metrics(face, path);
    spec.face(face, a, d)
}

/// Draws `text` in `family` at the preview size into `rect` (`line-height: 1`), cut with an
/// ellipsis to the rect's width (`text-overflow: ellipsis`).
pub fn draw_preview(painter: &Painter, rect: Rect, family: &str, text: &str, color: Color32) {
    if face_of(family).is_none() {
        draw_document_preview(painter, rect, family, text, color);
        return;
    }
    let size = rect.height();
    let spec = face_spec(family, size).line_height(size);
    text::draw_ellipsized(painter, rect, text, spec, color);
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn embedded_faces_have_no_instructions() {
        let bytes = crate::assets::embedded_font("inter/Inter-Variable.ttf").unwrap();
        let raw = ttf_parser::RawFace::parse(bytes, 0).unwrap();
        for tag in [b"fpgm", b"prep", b"cvt "] {
            let table = raw.table(ttf_parser::Tag::from_bytes(tag));
            assert!(table.is_none_or(<[u8]>::is_empty), "{tag:?}");
        }
        let maxp = raw.table(ttf_parser::Tag::from_bytes(b"maxp")).unwrap();
        assert_eq!(&maxp[26..28], [0, 0]);
        let face = ttf_parser::Face::parse(bytes, 0).unwrap();
        assert!(face.glyph_index('a').is_some());
        // The UI's Noto Sans keeps its instructions.
        let noto = crate::assets::embedded_font("noto-sans/NotoSans-Regular.ttf").unwrap();
        let raw = ttf_parser::RawFace::parse(noto, 0).unwrap();
        assert!(
            raw.table(ttf_parser::Tag::from_bytes(b"fpgm"))
                .is_some_and(|t| !t.is_empty())
        );
    }
}

/// Unhinted grayscale document previews are named separately from every chrome family.
pub fn install_document_previews(ctx: &egui::Context, defs: &mut FontDefinitions) {
    for (family, _) in crate::platform::fonts::choices(ctx).unwrap_or_default() {
        let Some(bytes) =
            pdn_core::fonts::registered_face(&family, pdn_core::fonts::FaceStyle::Regular)
        else {
            continue;
        };
        let name = format!("document-preview:{family}");
        defs.font_data.insert(
            name.clone(),
            Arc::new(FontData::from_owned(bytes.to_vec()).tweak(egui::FontTweak {
                hinting: Some(false),
                ..Default::default()
            })),
        );
        defs.families.insert(
            FontFamily::Name(name.clone().into()),
            vec![name, "noto-sans".into()],
        );
    }
}
pub fn draw_document_preview(
    painter: &Painter,
    rect: Rect,
    family: &str,
    value: &str,
    color: Color32,
) {
    let name = format!("document-preview:{family}");
    let family = if pdn_core::fonts::registered_face(family, pdn_core::fonts::FaceStyle::Regular)
        .is_some()
    {
        FontFamily::Name(name.into())
    } else {
        FontFamily::Proportional
    };
    let mut job = egui::text::LayoutJob::simple_singleline(
        value.into(),
        egui::FontId::new(20., family),
        color,
    );
    job.wrap.max_width = rect.width();
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = painter.layout_job(job);
    painter
        .with_clip_rect(rect.intersect(painter.clip_rect()))
        .galley(rect.min, galley, color);
    text::record_text_rect(painter.ctx(), rect.intersect(painter.clip_rect()));
}
