//! Port of: js/src/engine/util/FontManager.js (`bundledFonts`, `getTextToolChoices`)
//! Implements: ui-text-and-glyphs.md §2.2, §4 ("Plan: runtime selection"); spec §0.5
//! Reference: paintdotjs @ b4e6fc5
//!
//! Document font metadata and loading contracts. R47 keeps interface fonts bundled on every
//! target; native system fonts are available only to document text. Tests use Bundled policy.
use std::borrow::Cow;
use std::path::{Path, PathBuf};

pub static SELAWIK_REGULAR: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/font/selawik/selawk.ttf"
));
pub static SELAWIK_BOLD: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/font/selawik/selawkb.ttf"
));
/// Chromium draws paintdotjs's `font-weight: 600` text (`.tool-shape-grid-heading`) in it;
/// Paint.NET's own UI has no semibold text.
pub static SELAWIK_SEMIBOLD: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/font/selawik/selawksb.ttf"
));
pub static CARLITO_REGULAR: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/font/carlito/Carlito-Regular.ttf"
));
#[cfg(not(target_arch = "wasm32"))]
pub static CARLITO_BOLD: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/font/carlito/Carlito-Bold.ttf"
));
#[cfg(not(target_arch = "wasm32"))]
pub static CARLITO_ITALIC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/font/carlito/Carlito-Italic.ttf"
));
#[cfg(not(target_arch = "wasm32"))]
pub static CARLITO_BOLD_ITALIC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/font/carlito/Carlito-BoldItalic.ttf"
));

/// `System`: the native app (spec §0.5); `Bundled`: web, tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontPolicy {
    System,
    Bundled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FaceStyle {
    Regular,
    Bold,
    /// CSS weight 600 (Segoe UI Semibold, Selawik Semibold; Calibri and Carlito have none and
    /// use Bold, as CSS font matching does).
    Semibold,
    Italic,
    BoldItalic,
}

/// A resolved face. `family` is what the face is (`"Segoe UI"`, `"Selawik"`, `"Calibri"`,
/// `"Carlito"`); `system` is true when the bytes came from the OS font dirs.
#[derive(Clone, Debug)]
pub struct Face {
    pub family: &'static str,
    pub bytes: Cow<'static, [u8]>,
    pub system: bool,
}

/// OS/2 `usWinAscent`/`usWinDescent` and `head.unitsPerEm` (DirectWrite's line metrics).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinMetrics {
    pub units_per_em: u16,
    pub ascent: u16,
    pub descent: u16,
}

pub fn win_metrics(bytes: &[u8]) -> Option<WinMetrics> {
    let face = ttf_parser::Face::parse(bytes, 0).ok()?;
    let os2 = face.tables().os2?;
    Some(WinMetrics {
        units_per_em: face.units_per_em(),
        ascent: os2.windows_ascender() as u16,
        descent: os2.windows_descender().unsigned_abs(),
    })
}

/// The face carries TrueType instructions (`fpgm` or `prep`): Paint.NET's renderer hints it.
pub fn has_instructions(bytes: &[u8]) -> bool {
    ttf_parser::RawFace::parse(bytes, 0).is_ok_and(|raw| {
        [b"fpgm", b"prep"].iter().any(|t| {
            raw.table(ttf_parser::Tag::from_bytes(t))
                .is_some_and(|d| !d.is_empty())
        })
    })
}

/// Windows `%WINDIR%\Fonts` and the per-user dir; macOS `/Library/Fonts`, `/System/Library/Fonts`,
/// `~/Library/Fonts`; elsewhere `~/.local/share/fonts`, `~/.fonts`, `/usr/share/fonts`,
/// `/usr/local/share/fonts`. Empty on wasm.
pub fn os_font_dirs() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut dirs = vec![];
    if cfg!(target_arch = "wasm32") {
        return dirs;
    }
    if cfg!(windows) {
        dirs.extend(env("WINDIR").map(|w| w.join("Fonts")));
        dirs.extend(env("LOCALAPPDATA").map(|l| l.join("Microsoft").join("Windows").join("Fonts")));
    } else if cfg!(target_os = "macos") {
        dirs.push("/Library/Fonts".into());
        dirs.push("/System/Library/Fonts".into());
        dirs.extend(env("HOME").map(|h| h.join("Library").join("Fonts")));
    } else {
        if let Some(h) = env("HOME") {
            dirs.push(h.join(".local").join("share").join("fonts"));
            dirs.push(h.join(".fonts"));
        }
        dirs.push("/usr/share/fonts".into());
        dirs.push("/usr/local/share/fonts".into());
    }
    dirs
}

/// `bytes` is a static (non-variable) face whose name table says `family` / `subfamily`
/// (name ids 1 and 2). Paint.NET never uses "Segoe UI Variable" (`ui-text-and-glyphs.md §2.2`).
pub fn face_matches(bytes: &[u8], family: &str, subfamily: &str) -> bool {
    let Ok(face) = ttf_parser::Face::parse(bytes, 0) else {
        return false;
    };
    if face.tables().fvar.is_some() {
        return false;
    }
    // Any Unicode record: a face lists its subfamily in many languages ("Negreta", "Bold", …).
    let has = |id: u16, want: &str| {
        face.names()
            .into_iter()
            .any(|n| n.name_id == id && n.is_unicode() && n.to_string().as_deref() == Some(want))
    };
    has(ttf_parser::name_id::FAMILY, family) && has(ttf_parser::name_id::SUBFAMILY, subfamily)
}

/// Larger files are not fonts worth reading (and would stall startup).
pub const MAX_FONT_FILE: u64 = 64 * 1024 * 1024;

/// The regular files of at most 64 MB under `dirs` (recursively, depth ≤ 4), sorted: the
/// candidates `find_system_face` picks from. FIFOs, devices and unreadable entries are skipped.
/// Walk once and pass the list to every face lookup.
pub fn font_files(dirs: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            let Ok(meta) = std::fs::metadata(&p) else {
                continue;
            };
            if meta.is_dir() {
                if depth < 4 {
                    walk(&p, depth + 1, out);
                }
            } else if meta.is_file() && meta.len() <= MAX_FONT_FILE {
                out.push(p);
            }
        }
    }
    let mut files = vec![];
    for d in dirs {
        walk(d, 0, &mut files);
    }
    files.sort();
    files
}

fn read_font_file(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_FONT_FILE {
        return None;
    }
    let mut bytes = vec![];
    file.take(MAX_FONT_FILE + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_FONT_FILE).then_some(bytes)
}

/// The first of `candidates` (`font_files`) whose name equals one of `files` (ASCII
/// case-insensitive) and whose face matches. Unreadable files are skipped.
pub fn find_system_face(
    candidates: &[PathBuf],
    files: &[&str],
    family: &str,
    subfamily: &str,
) -> Option<Vec<u8>> {
    candidates
        .iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| files.iter().any(|f| f.eq_ignore_ascii_case(n)))
        })
        .find_map(|p| {
            let bytes = read_font_file(p)?;
            face_matches(&bytes, family, subfamily).then_some(bytes)
        })
}

/// `style`'s system file, its name-table family/subfamily, and the bundled stand-in.
type StyleEntry = (&'static str, &'static str, &'static str, &'static [u8]);

fn system_or_bundled(
    candidates: &[PathBuf],
    policy: FontPolicy,
    (file, family, subfamily, bundled): StyleEntry,
    system_family: &'static str,
    bundled_family: &'static str,
) -> Face {
    if policy == FontPolicy::System
        && let Some(bytes) = find_system_face(candidates, &[file], family, subfamily)
    {
        return Face {
            family: system_family,
            bytes: Cow::Owned(bytes),
            system: true,
        };
    }
    Face {
        family: bundled_family,
        bytes: Cow::Borrowed(bundled),
        system: false,
    }
}

/// Segoe UI from `candidates` (System policy), else Selawik (Bold for the bold styles; Selawik
/// ships no italic). `ui-text-and-glyphs.md §2.2`, spec §0.5.
pub fn ui_face_from(candidates: &[PathBuf], policy: FontPolicy, style: FaceStyle) -> Face {
    let entry: StyleEntry = match style {
        FaceStyle::Regular => ("segoeui.ttf", "Segoe UI", "Regular", SELAWIK_REGULAR),
        FaceStyle::Bold => ("segoeuib.ttf", "Segoe UI", "Bold", SELAWIK_BOLD),
        FaceStyle::Semibold => (
            "seguisb.ttf",
            "Segoe UI Semibold",
            "Regular",
            SELAWIK_SEMIBOLD,
        ),
        FaceStyle::Italic => ("segoeuii.ttf", "Segoe UI", "Italic", SELAWIK_REGULAR),
        FaceStyle::BoldItalic => ("segoeuiz.ttf", "Segoe UI", "Bold Italic", SELAWIK_BOLD),
    };
    system_or_bundled(candidates, policy, entry, "Segoe UI", "Selawik")
}

/// `ui_face_from` over the OS font dirs (one walk per call; under `Bundled` none).
pub fn ui_face(policy: FontPolicy, style: FaceStyle) -> Face {
    ui_face_from(&system_font_files(policy), policy, style)
}

/// `font_files(&os_font_dirs())` under `System`, nothing under `Bundled`.
pub fn system_font_files(policy: FontPolicy) -> Vec<PathBuf> {
    match policy {
        FontPolicy::System => font_files(&os_font_dirs()),
        FontPolicy::Bundled => vec![],
    }
}

/// Calibri from `candidates` (System policy), else Carlito (R23). On wasm only Carlito Regular is
/// embedded; the other styles fall back to it until Task 53's lazy loading.
pub fn calibri_face_from(candidates: &[PathBuf], policy: FontPolicy, style: FaceStyle) -> Face {
    #[cfg(target_arch = "wasm32")]
    if let Some(bytes) = registered_face("Carlito", style) {
        return Face {
            family: "Carlito",
            bytes: Cow::Owned(bytes.to_vec()),
            system: false,
        };
    }
    #[cfg(not(target_arch = "wasm32"))]
    let (bold, italic, bold_italic) = (CARLITO_BOLD, CARLITO_ITALIC, CARLITO_BOLD_ITALIC);
    #[cfg(target_arch = "wasm32")]
    let (bold, italic, bold_italic) = (CARLITO_REGULAR, CARLITO_REGULAR, CARLITO_REGULAR);
    let entry: StyleEntry = match style {
        FaceStyle::Regular => ("calibri.ttf", "Calibri", "Regular", CARLITO_REGULAR),
        FaceStyle::Bold | FaceStyle::Semibold => ("calibrib.ttf", "Calibri", "Bold", bold),
        FaceStyle::Italic => ("calibrii.ttf", "Calibri", "Italic", italic),
        FaceStyle::BoldItalic => ("calibriz.ttf", "Calibri", "Bold Italic", bold_italic),
    };
    system_or_bundled(candidates, policy, entry, "Calibri", "Carlito")
}

/// `calibri_face_from` over the OS font dirs (one walk per call; under `Bundled` none).
pub fn calibri_face(policy: FontPolicy, style: FaceStyle) -> Face {
    calibri_face_from(&system_font_files(policy), policy, style)
}

/// The bundled family a CSS family name renders in: the generics (ASCII case-insensitive) and the
/// two metric-compatible aliases.
pub fn resolve_family(family: &str) -> &str {
    const ALIASES: [(&str, &str); 8] = [
        ("sans-serif", "Noto Sans"),
        ("system-ui", "Noto Sans"),
        ("cursive", "Noto Sans"),
        ("fantasy", "Noto Sans"),
        ("serif", "Noto Serif"),
        ("monospace", "Noto Sans Mono"),
        ("Segoe UI", "Selawik"),
        ("Calibri", "Carlito"),
    ];
    ALIASES
        .iter()
        .find(|(g, _)| g.eq_ignore_ascii_case(family))
        .map_or(family, |(_, f)| f)
}

pub const BUNDLED: [(&str, &str); 24] = [
    ("Noto Sans", "noto-sans/NotoSans-Regular.ttf"),
    ("Inter", "inter/Inter-Variable.ttf"),
    (
        "Atkinson Hyperlegible",
        "atkinson-hyperlegible/AtkinsonHyperlegible-Regular.ttf",
    ),
    ("Lato", "lato/Lato-Regular.ttf"),
    ("Montserrat", "montserrat/Montserrat-Variable.ttf"),
    ("Open Sans", "open-sans/OpenSans-Variable.ttf"),
    ("Raleway", "raleway/Raleway-Variable.ttf"),
    ("Source Sans 3", "source-sans-3/SourceSans3-Variable.ttf"),
    ("Noto Serif", "noto-serif/NotoSerif-Regular.ttf"),
    ("Roboto Slab", "roboto-slab/RobotoSlab-Variable.ttf"),
    (
        "Playfair Display",
        "playfair-display/PlayfairDisplay-Variable.ttf",
    ),
    ("Merriweather", "merriweather/Merriweather-Variable.ttf"),
    (
        "Libre Baskerville",
        "libre-baskerville/LibreBaskerville-Variable.ttf",
    ),
    ("Source Serif 4", "source-serif-4/SourceSerif4-Variable.ttf"),
    ("Cinzel", "cinzel/Cinzel-Variable.ttf"),
    ("Oswald", "oswald/Oswald-Variable.ttf"),
    ("Bebas Neue", "bebas-neue/BebasNeue-Regular.ttf"),
    ("Caveat", "caveat/Caveat-Variable.ttf"),
    ("Pacifico", "pacifico/Pacifico-Regular.ttf"),
    (
        "Dancing Script",
        "dancing-script/DancingScript-Variable.ttf",
    ),
    ("Comic Neue", "comic-neue/ComicNeue-Regular.ttf"),
    ("Noto Sans Mono", "noto-sans-mono/NotoSansMono-Regular.ttf"),
    (
        "JetBrains Mono",
        "jetbrains-mono/JetBrainsMono-Variable.ttf",
    ),
    ("Inconsolata", "inconsolata/Inconsolata-Variable.ttf"),
];

/// R183 follow-up: lazily loaded script faces under `assets/fonts/` (family, file, the Unicode
/// blocks whose characters request it). Text draws a character its other faces lack with the
/// first registered one that has it; the Regular face also serves bold, which Text synthesizes.
/// The families are private names, so an installed font of the same name keeps its own faces.
pub const SCRIPT_FALLBACKS: [(&str, &str, Blocks); 12] = [
    (
        "paintrs fallback: Noto Sans Devanagari",
        "noto-sans-devanagari/NotoSansDevanagari-Regular.ttf",
        &[(0x0900, 0x097F), (0x1CD0, 0x1CFF), (0xA8E0, 0xA8FF)],
    ),
    (
        "paintrs fallback: Noto Sans Bengali",
        "noto-sans-bengali/NotoSansBengali-Regular.ttf",
        &[(0x0980, 0x09FF)],
    ),
    (
        "paintrs fallback: Noto Sans Gurmukhi",
        "noto-sans-gurmukhi/NotoSansGurmukhi-Regular.ttf",
        &[(0x0A00, 0x0A7F)],
    ),
    (
        "paintrs fallback: Noto Sans Gujarati",
        "noto-sans-gujarati/NotoSansGujarati-Regular.ttf",
        &[(0x0A80, 0x0AFF)],
    ),
    (
        "paintrs fallback: Noto Sans Oriya",
        "noto-sans-oriya/NotoSansOriya-Regular.ttf",
        &[(0x0B00, 0x0B7F)],
    ),
    (
        "paintrs fallback: Noto Sans Tamil",
        "noto-sans-tamil/NotoSansTamil-Regular.ttf",
        &[(0x0B80, 0x0BFF)],
    ),
    (
        "paintrs fallback: Noto Sans Telugu",
        "noto-sans-telugu/NotoSansTelugu-Regular.ttf",
        &[(0x0C00, 0x0C7F)],
    ),
    (
        "paintrs fallback: Noto Sans Kannada",
        "noto-sans-kannada/NotoSansKannada-Regular.ttf",
        &[(0x0C80, 0x0CFF)],
    ),
    (
        "paintrs fallback: Noto Sans Malayalam",
        "noto-sans-malayalam/NotoSansMalayalam-Regular.ttf",
        &[(0x0D00, 0x0D7F)],
    ),
    (
        "paintrs fallback: Noto Sans Sinhala",
        "noto-sans-sinhala/NotoSansSinhala-Regular.ttf",
        &[(0x0D80, 0x0DFF)],
    ),
    (
        "paintrs fallback: Noto Sans Thai",
        "noto-sans-thai/NotoSansThai-Regular.ttf",
        &[(0x0E00, 0x0E7F)],
    ),
    // Subset by scripts/fonts/subset_cjk.py: Hangul, Kana and common Han, no layout tables.
    // ponytail: rarer Han (Extension A, Big5-only) in these blocks loads the face and still draws
    // .notdef; the user approved this subset over the 16 MB full face.
    (
        "paintrs fallback: Noto Sans CJK SC",
        "noto-sans-cjk-sc/NotoSansCJKsc-Regular.otf",
        &[
            (0x1100, 0x11FF),
            (0x2E80, 0x9FFF),
            (0xA960, 0xA97F),
            (0xAC00, 0xD7FF),
            (0xF900, 0xFAFF),
            (0xFE30, 0xFE4F),
            (0xFF00, 0xFFEF),
        ],
    ),
];

/// Inclusive Unicode code point ranges.
pub type Blocks = &'static [(u32, u32)];

/// The `SCRIPT_FALLBACKS` family whose blocks hold `c`.
pub fn script_fallback(c: char) -> Option<&'static str> {
    SCRIPT_FALLBACKS
        .iter()
        .find(|(_, _, blocks)| {
            blocks
                .iter()
                .any(|&(lo, hi)| (lo..=hi).contains(&(c as u32)))
        })
        .map(|(family, ..)| *family)
}

/// Whether `family` is one of the `SCRIPT_FALLBACKS` (R195: best-effort, never required).
pub fn is_script_fallback(family: &str) -> bool {
    SCRIPT_FALLBACKS
        .iter()
        .any(|(f, ..)| f.eq_ignore_ascii_case(family))
}

/// `FontManager.bundledFonts` families, in order.
pub const BUNDLED_FAMILIES: [&str; 24] = {
    let mut names = [""; 24];
    let mut i = 0;
    while i < BUNDLED.len() {
        names[i] = BUNDLED[i].0;
        i += 1;
    }
    names
};

pub fn bundled_families() -> &'static [&'static str] {
    &BUNDLED_FAMILIES
}

/// `getTextToolChoices()`: (family, display name).
pub fn text_tool_choices() -> Vec<(&'static str, &'static str)> {
    BUNDLED_FAMILIES
        .iter()
        .map(|f| (*f, *f))
        .chain([
            ("sans-serif", "System Sans Serif"),
            ("serif", "System Serif"),
            ("monospace", "System Monospace"),
        ])
        .collect()
}

thread_local! {
    static DOCUMENT_CHOICES: std::cell::RefCell<Vec<(String, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// R188: the document font list (Task 53's (family, display name) collection) the UI thread's
/// font manager last published for the font drop-downs; `text_tool_choices` before it has.
pub fn document_choices() -> Vec<(String, String)> {
    let published = DOCUMENT_CHOICES.with(|c| c.borrow().clone());
    if published.is_empty() {
        text_tool_choices()
            .into_iter()
            .map(|(f, n)| (f.into(), n.into()))
            .collect()
    } else {
        published
    }
}

/// Publishes `document_choices` (the UI thread's font manager, as it syncs).
pub fn set_document_choices(choices: Vec<(String, String)>) {
    DOCUMENT_CHOICES.with(|c| *c.borrow_mut() = choices);
}

#[cfg(test)]
mod face_tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_dir(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("paintrs-fonts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        for (file, bytes) in files {
            std::fs::write(dir.join("nested").join(file), bytes).unwrap();
        }
        dir
    }

    /// `bytes` with its `name` table replaced by Windows Unicode records for ids 1 and 2: a
    /// stand-in for a system face named `family` / `subfamily` (no proprietary bytes committed).
    fn renamed(bytes: &[u8], family: &str, subfamily: &str) -> Vec<u8> {
        let utf16 = |s: &str| -> Vec<u8> { s.encode_utf16().flat_map(u16::to_be_bytes).collect() };
        let (f, sf) = (utf16(family), utf16(subfamily));
        let mut name: Vec<u8> = [0u16, 2, 6 + 2 * 12]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        for (id, off, len) in [(1, 0, f.len()), (2, f.len(), sf.len())] {
            for v in [3, 1, 0x409, id, len as u16, off as u16] {
                name.extend(v.to_be_bytes());
            }
        }
        name.extend(&f);
        name.extend(&sf);
        let mut out = bytes.to_vec();
        out.resize(out.len().next_multiple_of(4), 0);
        let offset = out.len() as u32;
        out.extend(&name);
        let tables = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
        let rec = (0..tables)
            .map(|i| 12 + 16 * i)
            .find(|&r| &bytes[r..r + 4] == b"name")
            .unwrap();
        out[rec + 8..rec + 12].copy_from_slice(&offset.to_be_bytes());
        out[rec + 12..rec + 16].copy_from_slice(&(name.len() as u32).to_be_bytes());
        out
    }

    const INTER_VARIABLE: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/font/inter/Inter-Variable.ttf"
    ));

    /// Review Focus 5: a font dir with only a variable face or a corrupt file named like Segoe UI.
    #[test]
    fn system_font_rejects_variable_and_corrupt_faces() {
        let dir = fixture_dir(
            "reject",
            &[
                ("segoeui.ttf", INTER_VARIABLE),
                ("segoeuib.ttf", &b"not a font"[..]),
                ("SELAWK.TTF", SELAWIK_REGULAR),
            ],
        );
        let files = font_files(std::slice::from_ref(&dir));
        assert!(find_system_face(&files, &["segoeui.ttf"], "Segoe UI", "Regular").is_none());
        assert!(find_system_face(&files, &["segoeuib.ttf"], "Segoe UI", "Bold").is_none());
        // File names match case-insensitively; the name table decides.
        assert!(find_system_face(&files, &["selawk.ttf"], "Selawik", "Regular").is_some());
        assert!(find_system_face(&files, &["selawk.ttf"], "Segoe UI", "Regular").is_none());
        let face = ui_face_from(&files, FontPolicy::System, FaceStyle::Regular);
        assert_eq!((face.family, face.system), ("Selawik", false));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bundled_policy_never_reads_the_system() {
        let segoe = renamed(SELAWIK_REGULAR, "Segoe UI", "Regular");
        let segoe_bold = renamed(SELAWIK_BOLD, "Segoe UI", "Bold");
        let segoe_semibold = renamed(SELAWIK_SEMIBOLD, "Segoe UI Semibold", "Regular");
        let calibri = renamed(CARLITO_REGULAR, "Calibri", "Regular");
        let dir = fixture_dir(
            "bundled",
            &[
                ("segoeui.ttf", &segoe),
                ("segoeuib.ttf", &segoe_bold),
                ("seguisb.ttf", &segoe_semibold),
                ("calibri.ttf", &calibri),
            ],
        );
        let files = font_files(std::slice::from_ref(&dir));
        // The fixture is a system Segoe UI and Calibri: `System` picks them...
        for (style, bytes) in [
            (FaceStyle::Regular, &segoe),
            (FaceStyle::Bold, &segoe_bold),
            (FaceStyle::Semibold, &segoe_semibold),
        ] {
            let face = ui_face_from(&files, FontPolicy::System, style);
            assert_eq!((face.family, face.system), ("Segoe UI", true), "{style:?}");
            assert_eq!(&*face.bytes, &bytes[..], "{style:?}");
        }
        let face = calibri_face_from(&files, FontPolicy::System, FaceStyle::Regular);
        assert_eq!((face.family, face.system), ("Calibri", true));
        // ...and `Bundled` ignores them.
        for (style, bytes) in [
            (FaceStyle::Regular, SELAWIK_REGULAR),
            (FaceStyle::Bold, SELAWIK_BOLD),
            (FaceStyle::Semibold, SELAWIK_SEMIBOLD),
        ] {
            let face = ui_face_from(&files, FontPolicy::Bundled, style);
            assert_eq!((face.family, face.system), ("Selawik", false), "{style:?}");
            assert_eq!(&*face.bytes, bytes, "{style:?}");
        }
        let face = calibri_face_from(&files, FontPolicy::Bundled, FaceStyle::Regular);
        assert_eq!((face.family, &*face.bytes), ("Carlito", CARLITO_REGULAR));
        // Selawik has no italic: Italic falls back to Regular (egui synthesizes the slant).
        assert_eq!(
            &*ui_face_from(&[], FontPolicy::Bundled, FaceStyle::Italic).bytes,
            SELAWIK_REGULAR
        );
        assert!(system_font_files(FontPolicy::Bundled).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The trust boundary: only regular files of at most 64 MB are candidates.
    #[test]
    fn font_files_skip_directories_and_oversize_files() {
        let segoe = renamed(SELAWIK_REGULAR, "Segoe UI", "Regular");
        let segoe_bold = renamed(SELAWIK_BOLD, "Segoe UI", "Bold");
        let dir = fixture_dir("trust", &[]);
        // A directory named like the face; the real face sits inside it.
        let fake = dir.join("nested").join("segoeui.ttf");
        std::fs::create_dir_all(&fake).unwrap();
        std::fs::write(fake.join("segoeui.ttf"), &segoe).unwrap();
        // A valid face padded past 64 MB (sparse).
        let big = dir.join("nested").join("segoeuib.ttf");
        std::fs::write(&big, &segoe_bold).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&big)
            .unwrap()
            .set_len(MAX_FONT_FILE + 1)
            .unwrap();
        let files = font_files(std::slice::from_ref(&dir));
        assert_eq!(files, [fake.join("segoeui.ttf")]);
        let regular = find_system_face(&files, &["segoeui.ttf"], "Segoe UI", "Regular");
        assert_eq!(regular.as_deref(), Some(&segoe[..]));
        assert!(find_system_face(&files, &["segoeuib.ttf"], "Segoe UI", "Bold").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn generics_and_aliases_resolve_to_bundled_faces() {
        assert_eq!(resolve_family("sans-serif"), "Noto Sans");
        assert_eq!(resolve_family("System-UI"), "Noto Sans");
        assert_eq!(resolve_family("serif"), "Noto Serif");
        assert_eq!(resolve_family("monospace"), "Noto Sans Mono");
        assert_eq!(resolve_family("cursive"), "Noto Sans");
        assert_eq!(resolve_family("Segoe UI"), "Selawik");
        assert_eq!(resolve_family("calibri"), "Carlito");
        assert_eq!(resolve_family("Inter"), "Inter");
    }

    #[test]
    fn selawik_has_segoe_ui_win_metrics_and_no_instructions() {
        // ui-text-and-glyphs.md §3: Segoe UI upem 2048, win ascent 2210, descent 514.
        assert_eq!(
            win_metrics(SELAWIK_REGULAR),
            Some(WinMetrics {
                units_per_em: 2048,
                ascent: 2210,
                descent: 514
            })
        );
        assert!(!has_instructions(SELAWIK_REGULAR));
        assert!(has_instructions(CARLITO_REGULAR));
    }
}

#[cfg(test)]
mod manager_tests {
    use super::*;
    #[test]
    fn validates_bounded_real_fonts_and_explicitly_rejects_woff() {
        let font = include_bytes!("../../../assets/font/inter/Inter-Variable.ttf");
        assert_eq!(validate_font(font).unwrap().family, "Inter");
        for bytes in [
            b"not a font".as_slice(),
            &font[..100],
            b"wOFFbad",
            b"wOF2bad",
            b"",
        ] {
            assert!(validate_font(bytes).is_err());
        }
        assert_eq!(
            validate_font(b"wOF2bad").unwrap_err().to_string(),
            "Unsupported font format"
        );
        assert_eq!(BUNDLED.len(), 24);
        for (_, path) in BUNDLED {
            let bytes = std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../assets/font")
                    .join(path),
            )
            .unwrap();
            assert!(validate_font(&bytes).is_ok(), "{path}");
        }
    }
    #[test]
    fn system_names_and_faces_are_policy_gated_and_sorted() {
        let dir =
            std::env::temp_dir().join(format!("paintrs-task53-enumeration-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (file, bytes) in [
            ("a.ttf", SELAWIK_REGULAR),
            ("b.ttf", SELAWIK_BOLD),
            ("c.ttf", CARLITO_REGULAR),
            ("bad.ttf", b"bad".as_slice()),
        ] {
            std::fs::write(dir.join(file), bytes).unwrap();
        }
        assert_eq!(
            system_families(std::slice::from_ref(&dir), FontPolicy::System),
            ["Carlito", "Selawik"]
        );
        assert!(system_families(std::slice::from_ref(&dir), FontPolicy::Bundled).is_empty());
        assert!(
            document_face_from(
                &font_files(std::slice::from_ref(&dir)),
                FontPolicy::System,
                "Selawik",
                FaceStyle::Bold
            )
            .is_some()
        );
        assert!(
            document_face_from(
                &font_files(std::slice::from_ref(&dir)),
                FontPolicy::Bundled,
                "Selawik",
                FaceStyle::Bold
            )
            .is_none()
        );
        assert!(
            document_face_from(
                &font_files(std::slice::from_ref(&dir)),
                FontPolicy::System,
                "Missing family",
                FaceStyle::Regular
            )
            .is_none()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn collection_enumerates_and_resolves_the_second_family() {
        // Two original faces in a TTC. Table bytes stay unchanged; directory offsets become absolute.
        let mut data = b"ttcf\0\x01\0\0\0\0\0\x02".to_vec();
        data.extend([0; 8]);
        for (i, font) in [SELAWIK_REGULAR, CARLITO_REGULAR].into_iter().enumerate() {
            while !data.len().is_multiple_of(4) {
                data.push(0);
            }
            let offset = data.len();
            data[12 + i * 4..16 + i * 4].copy_from_slice(&(offset as u32).to_be_bytes());
            let mut face = font.to_vec();
            let n = u16::from_be_bytes([face[4], face[5]]) as usize;
            for table in 0..n {
                let p = 12 + table * 16 + 8;
                let old = u32::from_be_bytes(face[p..p + 4].try_into().unwrap());
                face[p..p + 4].copy_from_slice(&(old + offset as u32).to_be_bytes());
            }
            data.extend(face);
        }
        let dir =
            std::env::temp_dir().join(format!("paintrs-task53-collection-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("two.ttc");
        std::fs::write(&path, &data).unwrap();
        assert_eq!(
            system_families(std::slice::from_ref(&dir), FontPolicy::System),
            ["Carlito", "Selawik"]
        );
        let resolved =
            document_face_from(&[path], FontPolicy::System, "Carlito", FaceStyle::Regular).unwrap();
        assert_eq!(validate_font(&resolved).unwrap().family, "Carlito");
        let original = ttf_parser::Face::parse(CARLITO_REGULAR, 0).unwrap();
        let actual = ttf_parser::Face::parse(&resolved, 0).unwrap();
        for tag in [b"glyf", b"fpgm", b"prep", b"hmtx"] {
            assert_eq!(
                original.raw_face().table(ttf_parser::Tag::from_bytes(tag)),
                actual.raw_face().table(ttf_parser::Tag::from_bytes(tag))
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn registered_styles_are_optional_and_removable() {
        let name = "Task53 style test";
        assert!(registered_face(name, FaceStyle::Bold).is_none());
        register_lazy_face(name, FaceStyle::Regular, SELAWIK_REGULAR.to_vec()).unwrap();
        register_lazy_face(name, FaceStyle::Bold, SELAWIK_BOLD.to_vec()).unwrap();
        assert_eq!(
            &*registered_face(name, FaceStyle::Bold).unwrap(),
            SELAWIK_BOLD
        );
        assert_eq!(
            &*registered_face(name, FaceStyle::Italic).unwrap(),
            SELAWIK_REGULAR
        );
        assert!(register_lazy_face(name, FaceStyle::Regular, b"bad".to_vec()).is_err());
        assert_eq!(
            &*registered_face(name, FaceStyle::Regular).unwrap(),
            SELAWIK_REGULAR
        );
        unregister_family(name);
        assert!(registered_face(name, FaceStyle::Bold).is_none());
    }
}

/// User-visible validation failures. WOFF containers require a decoder, not just Brotli.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontError {
    Unsupported,
    Empty,
    TooLarge,
    Invalid,
}
impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unsupported => "Unsupported font format",
            Self::Empty => "The font file is empty",
            Self::TooLarge => "The font file is too large",
            Self::Invalid => "The font file is invalid",
        })
    }
}
impl std::error::Error for FontError {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontMeta {
    pub family: String,
    pub style: FaceStyle,
}
/// Validate original sfnt bytes before storage or registration. No font bytes enter prefs.
pub fn validate_font(bytes: &[u8]) -> Result<FontMeta, FontError> {
    if bytes.is_empty() {
        return Err(FontError::Empty);
    }
    if bytes.len() as u64 > MAX_FONT_FILE {
        return Err(FontError::TooLarge);
    }
    if bytes.starts_with(b"wOFF") || bytes.starts_with(b"wOF2") {
        return Err(FontError::Unsupported);
    }
    let face = ttf_parser::Face::parse(bytes, 0).map_err(|_| FontError::Invalid)?;
    if face.number_of_glyphs() == 0 || face.tables().cmap.is_none() {
        return Err(FontError::Invalid);
    }
    face_meta(&face)
}
fn face_meta(face: &ttf_parser::Face<'_>) -> Result<FontMeta, FontError> {
    let family = face
        .names()
        .into_iter()
        .filter(|n| n.name_id == ttf_parser::name_id::FAMILY)
        .filter_map(|n| n.to_string())
        .find(|n| !n.trim().is_empty())
        .ok_or(FontError::Invalid)?;
    let style = match (face.is_bold(), face.is_italic()) {
        (true, true) => FaceStyle::BoldItalic,
        (true, false) => FaceStyle::Bold,
        (false, true) => FaceStyle::Italic,
        _ => FaceStyle::Regular,
    };
    Ok(FontMeta { family, style })
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontRecord {
    pub id: String,
    pub name: String,
    pub family: String,
    pub file_name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub size: u64,
    pub added_at: u64,
}
impl FontRecord {
    pub fn from_bytes(
        id: String,
        file_name: String,
        bytes: &[u8],
        added_at: u64,
    ) -> Result<Self, FontError> {
        validate_font(bytes)?;
        let name = std::path::Path::new(&file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Custom Font")
            .trim();
        Ok(Self {
            family: format!("paintjs-font-{id}"),
            id,
            name: if name.is_empty() {
                "Custom Font".into()
            } else {
                name.into()
            },
            file_name,
            kind: if bytes.starts_with(b"OTTO") {
                "font/otf"
            } else {
                "font/ttf"
            }
            .into(),
            size: bytes.len() as u64,
            added_at,
        })
    }
    pub fn safe_id(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 100
            && self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    }
    pub fn extension(&self) -> &'static str {
        if self.kind == "font/otf" {
            "otf"
        } else {
            "ttf"
        }
    }
}
#[derive(Default, Clone, Debug)]
pub struct FontRegistry {
    pub records: Vec<FontRecord>,
}
impl FontRegistry {
    pub fn choices(&self, system: &[String]) -> Vec<(String, String)> {
        system
            .iter()
            .map(|s| (s.clone(), s.clone()))
            .chain(
                text_tool_choices()
                    .into_iter()
                    .map(|(a, b)| (a.into(), b.into())),
            )
            .chain(
                self.records
                    .iter()
                    .map(|r| (r.family.clone(), r.name.clone())),
            )
            .collect()
    }
}
/// No fallback is chosen here. Text owns its native initial-family and glyph fallback rules.
pub fn registered_face(family: &str, style: FaceStyle) -> Option<std::sync::Arc<[u8]>> {
    crate::canvas::text::registered_style_font_bytes(family, style)
}
pub fn register_lazy_face(family: &str, style: FaceStyle, bytes: Vec<u8>) -> Result<(), FontError> {
    validate_font(&bytes)?;
    crate::canvas::text::register_font_style(family, style, bytes);
    Ok(())
}
pub fn unregister_family(family: &str) {
    crate::canvas::text::unregister_font(family);
}
/// Retain original font tables when adapting a selected TTC face to the single-face Text API.
/// Only sfnt directory offsets and the required head checksum adjustment change.
fn collection_face(bytes: &[u8], index: u32) -> Option<Vec<u8>> {
    if ttf_parser::fonts_in_collection(bytes).is_none() {
        return Some(bytes.to_vec());
    }
    let raw = ttf_parser::RawFace::parse(bytes, index).ok()?;
    let count = raw.table_records.len();
    if count == 0 || count > 4095 {
        return None;
    }
    let mut out = vec![0u8; 12 + count as usize * 16];
    let version = if raw.table(ttf_parser::Tag::from_bytes(b"CFF ")).is_some()
        || raw.table(ttf_parser::Tag::from_bytes(b"CFF2")).is_some()
    {
        *b"OTTO"
    } else {
        [0, 1, 0, 0]
    };
    out[..4].copy_from_slice(&version);
    out[4..6].copy_from_slice(&count.to_be_bytes());
    let selector = 15 - count.leading_zeros() as u16;
    let search = (1u16 << selector) * 16;
    out[6..8].copy_from_slice(&search.to_be_bytes());
    out[8..10].copy_from_slice(&selector.to_be_bytes());
    out[10..12].copy_from_slice(&(count * 16 - search).to_be_bytes());
    let mut head = None;
    fn checksum(bytes: &[u8]) -> u32 {
        bytes.chunks(4).fold(0u32, |sum, chunk| {
            let mut word = [0; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            sum.wrapping_add(u32::from_be_bytes(word))
        })
    }
    for (i, record) in raw.table_records.into_iter().enumerate() {
        let data = raw.table(record.tag)?;
        let offset = out.len();
        if offset.checked_add(data.len())? as u64 > MAX_FONT_FILE {
            return None;
        }
        out.extend_from_slice(data);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        if record.tag == ttf_parser::Tag::from_bytes(b"head") {
            if data.len() < 12 {
                return None;
            }
            out[offset + 8..offset + 12].fill(0);
            head = Some(offset + 8);
        }
        let p = 12 + i * 16;
        let check = checksum(&out[offset..offset + data.len()]);
        out[p..p + 4].copy_from_slice(&record.tag.to_bytes());
        out[p + 4..p + 8].copy_from_slice(&check.to_be_bytes());
        out[p + 8..p + 12].copy_from_slice(&(offset as u32).to_be_bytes());
        out[p + 12..p + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
    }
    let adjustment = 0xB1B0AFBAu32.wrapping_sub(checksum(&out));
    let head = head?;
    out[head..head + 4].copy_from_slice(&adjustment.to_be_bytes());
    Some(out)
}
fn face_indices(bytes: &[u8]) -> std::ops::Range<u32> {
    let count = ttf_parser::fonts_in_collection(bytes).unwrap_or(1);
    // Bound a corrupt collection's claimed count before parsing its face offsets.
    0..if count <= 4096 && count as usize <= bytes.len() / 4 {
        count
    } else {
        0
    }
}
/// First matching original system face, regular when the requested style is unavailable.
/// No host lookup under Bundled or wasm and no implicit stand-in or fallback.
pub fn document_face_from(
    candidates: &[PathBuf],
    policy: FontPolicy,
    family: &str,
    style: FaceStyle,
) -> Option<Vec<u8>> {
    if policy == FontPolicy::Bundled || cfg!(target_arch = "wasm32") {
        return None;
    }
    let mut regular = None;
    for path in candidates {
        let Some(bytes) = read_font_file(path) else {
            continue;
        };
        for index in face_indices(&bytes) {
            let Ok(face) = ttf_parser::Face::parse(&bytes, index) else {
                continue;
            };
            let Ok(meta) = face_meta(&face) else { continue };
            if !meta.family.eq_ignore_ascii_case(family) {
                continue;
            }
            if meta.style == style
                || (style == FaceStyle::Semibold && meta.style == FaceStyle::Bold)
            {
                return collection_face(&bytes, index);
            }
            if meta.style == FaceStyle::Regular && regular.is_none() {
                regular = collection_face(&bytes, index);
            }
        }
    }
    regular
}
/// Re-scans per call; cache it if a document-font picker opens slowly.
pub fn system_families(dirs: &[PathBuf], policy: FontPolicy) -> Vec<String> {
    if policy == FontPolicy::Bundled || cfg!(target_arch = "wasm32") {
        return vec![];
    }
    let mut names = Vec::new();
    for path in font_files(dirs) {
        let Some(bytes) = read_font_file(&path) else {
            continue;
        };
        for index in face_indices(&bytes) {
            let Ok(face) = ttf_parser::Face::parse(&bytes, index) else {
                continue;
            };
            let Ok(meta) = face_meta(&face) else { continue };
            if !meta.family.starts_with('@') {
                names.push(meta.family);
            }
        }
    }
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}
