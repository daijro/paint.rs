//! R183 follow-up: under ja-JP, he-IL, hi-IN, ar-EG and th-TH the Calendar requests the lazy
//! script faces its culture's names need and, with them registered, draws no .notdef. Its own
//! test process: the culture and the registered faces are global.
use pdn_core::canvas::ImageData;
use pdn_core::effect::bb::calendar::{bold_when_normal, date_profile};
use pdn_core::effect::bb::conventions::SANS_FONT;
use pdn_core::effect::bb::indirect_defaults;
use pdn_core::effect::bb::text::{glyph_run, pt_to_px};
use pdn_core::effect::rng::CivilDate;
use pdn_core::effect::{EffectEnv, apply, find};
use pdn_core::fonts::{FaceStyle, SCRIPT_FALLBACKS, register_lazy_face};

/// The characters `text` draws as .notdef (glyph 0).
fn notdefs(text: &str, px: f32, bold: bool) -> String {
    let run = glyph_run(text, SANS_FONT, px, bold, [0.0; 2]);
    run.layout
        .lines
        .iter()
        .flat_map(|l| {
            let units: Vec<u16> = l.text.encode_utf16().collect();
            l.glyphs
                .iter()
                .filter(|g| g.glyph_id == 0)
                .map(move |g| String::from_utf16_lossy(&units[g.utf16.clone()]))
        })
        .collect()
}

#[test]
fn calendar_names_draw_without_notdef() {
    let def = find("bbCalendar").unwrap();
    let ui = def.indirect.unwrap();
    let src = ImageData {
        width: 700,
        height: 500,
        data: vec![0; 700 * 500 * 4],
    };
    for (culture, want) in [
        ("ja-JP", &["paintrs fallback: Noto Sans CJK SC"][..]),
        ("he-IL", &[]),
        ("hi-IN", &["paintrs fallback: Noto Sans Devanagari"]),
        ("ar-EG", &[]),
        ("th-TH", &["paintrs fallback: Noto Sans Thai"]),
    ] {
        pdn_core::i18n::with_global(|g| g.set_culture(culture));
        let mut env = EffectEnv::whole(700, 500);
        env.culture = culture;
        env.today = CivilDate::new(2026, 1, 10);
        let v = indirect_defaults(&def, &env);
        let lazy = || -> Vec<String> {
            (ui.fonts)(&v)
                .into_iter()
                .filter(|(f, _)| SCRIPT_FALLBACKS.iter().any(|(s, ..)| s == f))
                .map(|(f, _)| f)
                .collect()
        };
        assert_eq!(lazy(), want, "{culture}");
        let before = apply(&src, &def, &v, &env);
        for (family, file, _) in SCRIPT_FALLBACKS.iter().filter(|(f, ..)| want.contains(f)) {
            let path = format!("{}/../../assets/fonts/{file}", env!("CARGO_MANIFEST_DIR"));
            register_lazy_face(family, FaceStyle::Regular, std::fs::read(path).unwrap()).unwrap();
        }
        assert!(lazy().is_empty(), "{culture}");
        if !want.is_empty() {
            // The title and weekday row now draw the script face's glyphs, not missing boxes.
            assert_ne!(apply(&src, &def, &v, &env).data, before.data, "{culture}");
        }
        // Every title and weekday name the Calendar can draw, in its main font (Plan::new).
        let names = date_profile(culture).names;
        let px = pt_to_px(v["size"].as_f64().unwrap() as f32);
        let bold = bold_when_normal(SANS_FONT);
        let titles = names.months.iter().map(|m| format!("{m} 2026"));
        for text in titles
            .chain(names.days.clone())
            .chain(names.abbreviated_days.clone())
        {
            assert_eq!(notdefs(&text, px, bold), "", "{culture} {text}");
        }
    }
}
