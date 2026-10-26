//! R183 follow-up: Text's lazy script fallback faces (`fonts::SCRIPT_FALLBACKS`) after Noto Sans
//! and R131's Arabic/Hebrew. Its own test process: registered faces are global.
use pdn_core::effect::bb::text::glyph_run;
use pdn_core::fonts::{
    FaceStyle, FontPolicy, SCRIPT_FALLBACKS, register_lazy_face, script_fallback,
};
use pdn_core::tool::text::{
    DrawItem, FontSizeMetric, TextAlign, TextRenderingMode, TextSettings, script_fallbacks,
};

/// The characters `text` draws as .notdef (glyph 0) in the default Selawik.
fn notdefs(text: &str) -> String {
    let run = glyph_run(text, "Selawik", 16.0, false, [0.0; 2]);
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

/// Whether `text` draws a colour bitmap.
fn colour(text: &str) -> bool {
    let run = glyph_run(text, "Selawik", 16.0, false, [0.0; 2]);
    run.layout.lines[0]
        .items
        .iter()
        .any(|i| matches!(i, DrawItem::Bitmap(_)))
}

#[test]
fn script_faces_replace_missing_glyphs() {
    let s = TextSettings {
        family: "Selawik".into(),
        size: 12.0,
        metric: FontSizeMetric::Fixed96Dpi,
        bold: false,
        italic: false,
        underline: false,
        strikeout: false,
        mode: TextRenderingMode::Smooth,
        align: TextAlign::Left,
        antialias: true,
        policy: FontPolicy::Bundled,
        ..Default::default()
    };
    let samples = [
        "1月 日曜日 あ",
        "ינואר",
        "जनवरी",
        "يناير",
        "มกราคม",
        "1월 일요일",
    ];
    // Hebrew and Arabic come from the static R131 faces; the rest wait for their lazy faces.
    assert_eq!(notdefs(samples[1]) + &notdefs(samples[3]), "");
    assert_eq!(notdefs("1月"), "月");
    assert_eq!(
        script_fallbacks(&samples.join("\n"), &s),
        [
            "paintrs fallback: Noto Sans CJK SC",
            "paintrs fallback: Noto Sans Devanagari",
            "paintrs fallback: Noto Sans Thai"
        ]
    );
    // R117: colour-capable scalars the CJK subset also has never request it, unless VS15 does.
    assert!(script_fallbacks("♥㊗〰↔", &s).is_empty());
    assert_eq!(
        script_fallbacks("㊗\u{fe0e}", &s),
        ["paintrs fallback: Noto Sans CJK SC"]
    );
    for (family, file, blocks) in SCRIPT_FALLBACKS {
        let path = format!("{}/../../assets/fonts/{file}", env!("CARGO_MANIFEST_DIR"));
        let bytes = std::fs::read(&path).unwrap();
        register_lazy_face(family, FaceStyle::Regular, bytes).unwrap();
        // Each face draws a letter of its first block (KA, TO TAO, a Hangul jamo).
        let c = char::from_u32(blocks[0].0 + 0x15).unwrap();
        assert_eq!(script_fallback(c), Some(family));
        assert_eq!(notdefs(&c.to_string()), "", "{family}");
    }
    for text in samples {
        assert_eq!(notdefs(text), "", "{text}");
    }
    assert!(script_fallbacks(&samples.join("\n"), &s).is_empty());
    // With CJK and the colour face both registered, ♥ and ㊗ stay colour; VS15 draws CJK's outline.
    let path = format!(
        "{}/../../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    register_lazy_face(
        "Noto Color Emoji",
        FaceStyle::Regular,
        std::fs::read(path).unwrap(),
    )
    .unwrap();
    assert!(colour("♥") && colour("㊗"));
    assert!(!colour("㊗\u{fe0e}"));
    assert_eq!(notdefs("㊗\u{fe0e}"), "");
}
