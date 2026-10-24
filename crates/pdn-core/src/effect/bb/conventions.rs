//! BB spec: common.md §1, §5.1, §5.2, §5.4, §5.7, §5.9
//!
//! The uncertain Paint.NET building-block choices, one constant each (user rule). Change a value
//! here, never at a call site.
use super::staircase::ByteConv;
use crate::effect::RenderInfo;
use crate::effect::surface::{Alpha, Edge, Space};

// spec-question: Q1 Default GPU colour context
/// common.md §1 UNCERTAIN: the context of a GPU effect that sets none. Paint.NET's default is
/// linear light, premultiplied.
pub const GPU_CONTEXT: RenderInfo = RenderInfo::new(Space::Linear, Alpha::Premultiplied);
// spec-question: Q1 Default GPU colour context
/// R138: native Flames 6.0 ramp/smoke relationships support encoded sRGB for this effect only.
/// Evidence: native-observations/2026-10-21-flames/native-analysis.md; global GPU_CONTEXT is unchanged.
pub const FLAMES_GPU_CONTEXT: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Premultiplied);
// spec-question: Q39 Transparent Flames swizzle alpha mode
/// R138: preserve saturated RGB as premultiplied for the replaced alpha; exact native graph unknown.
pub const FLAMES_SWIZZLE_ALPHA: Alpha = Alpha::Premultiplied;
// spec-question: Q40 Transparent Flames clamp before saturation
/// R138: native opaque-gradient evidence supports clamping the ramp before existing HSL150.
pub const FLAMES_CLAMP_BEFORE_SATURATION: bool = true;
// spec-question: Q41 Polygon thin-DDA closing endpoint evaluation
/// R139: degree-first, unreduced target indices reproduce the complete native 21/48 masks.
/// False restores modulo targets. Other strokes retain cached vertices; no cardinal snapping.
pub const POLYGON_DDA_UNWRAPPED_TARGET: bool = true;
/// The other column of the spec's gamma/linear tables (tests only switch to it through `test_util::bb_ctx`).
pub const ALT_GPU_CONTEXT: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Premultiplied);
// spec-question: Q3 Black and White+ flood colour conversion
/// Convert blend-colour bytes into the effective working space (true), rather than pass k/255
/// as working-space values (false), so Multiply on white reproduces the selected colour.
pub const BW_FLOOD_COLOUR_MANAGED: bool = true;
// spec-question: Q26 Hue/Saturation+ zero-alpha intermediate colour
/// Follow the explicit premultiply/unpremultiply topology: zero analysis H/S/L and chosen adjusted
/// RGB at alpha zero (true), or preserve hidden intermediate colour through those steps (false).
pub const HUE_PREMULTIPLIED_INTERMEDIATES: bool = true;
// spec-question: Q29 Blur Selection Edge outside-band stored colour
/// R112: the explicit unchanged complement keeps stored RGBA, including RGB at alpha zero.
/// False models a full-image premultiply/unpremultiply roundtrip instead; visible band maths is identical.
pub const BLUR_SELECTION_EDGE_PRESERVE_OUTSIDE_BAND: bool = true;
/// common.md §5.1: Paint.NET's radius → σ helper.
pub const RADIUS_TO_SIGMA: f64 = 1.0 / 3.0;
/// Added to every angle given to the Drop Shadow building block (−45° = shadow down-right; common.md §5.9).
pub const SHADOW_ANGLE_OFFSET_DEG: f64 = 0.0;
/// Added to every angle given to the Motion Blur smear (Inner Shadow, Long Shadow; object-inner-shadow.md Notes).
pub const SMEAR_ANGLE_OFFSET_DEG: f64 = 0.0;
/// Added to every Emboss angle (the dial points away from the light; object-bevel-object.md step 4).
pub const EMBOSS_ANGLE_OFFSET_DEG: f64 = 0.0;
/// Drop Shadow+ Tilt components 1, 2, 3 → Rotate/Zoom keys (object-drop-shadow-plus.md "3D Shadow tab" step 2).
pub const ROTATE_ZOOM_TILT_KEYS: [&str; 3] = ["angle", "rollDirection", "rollAmount"];
/// Drop Shadow+ Location (−1…1) → Rotate/Zoom pan units.
pub const ROTATE_ZOOM_PAN_SCALE: f64 = 1.0;
/// Rotate/Zoom edge-mode option 3 (inferred "transparent outside").
pub const ROTATE_ZOOM_EDGE: &str = "transparent";
/// Rotate/Zoom quality option 2.
pub const ROTATE_ZOOM_QUALITY: f64 = 2.0;
// spec-question: Q10 Classic Median "k-th smallest"
/// Whether the classic Median's "k-th smallest" counts from 1 (v[k − 1]) or from 0 (v[k]).
pub const MEDIAN_RANK_BASE: usize = 1;
// spec-question: Q15 Classic Hue/Saturation op's HSV round trip
/// The classic Hue/Saturation op ends with Paint.NET's integer HSV round trip (`HsvColor`,
/// truncating both ways).
pub const CLASSIC_HSV_ROUND_TRIP: bool = true;
// spec-question: Q17 Pastel recolour table vs formula
/// The pastel value offset as the plugin computes it, 1 − 0.3 + 0.1: one ulp below 0.8, so black
/// becomes 203. The step-1 table's 150 and 153 rows fit 0.8 itself, which breaks its 0, 3, 30, 51
/// and 102 rows.
pub const PASTEL_VALUE_OFFSET: f64 = 1.0 - 0.3 + 0.1;
// spec-question: Q18 Direct2D Vignette on semi-transparent pixels
/// The Direct2D Vignette's source-over of its colour: `Premultiplied` composites on premultiplied
/// pixels (alpha rises to cov + a·(1 − cov)); `Straight` mixes the straight colour and keeps alpha.
pub const VIGNETTE_ALPHA_MODE: Alpha = Alpha::Premultiplied;
// spec-question: Q19 transparent mask pixels: stored colour
/// Gray masks use stored straight RGB even at alpha zero (true), or treat alpha-zero mask pixels
/// as black (false). Partial alpha never scales the mask luma (object-apply-mask.md Notes).
pub const APPLY_MASK_STORED_TRANSPARENT_COLOUR: bool = true;
// spec-question: Q20 Classic Gaussian blur edges
/// The classic Gaussian's off-canvas taps are left out with the weights renormalised (false) or
/// read from the nearest edge pixel (true).
pub const CLASSIC_BLUR_CLAMP_EDGES: bool = false;
// spec-question: Q21 Pastel recolour hue arithmetic
/// The pastel recolour's hue and saturation come from the 0–255 channel values (true) or from
/// c/255 (false). The two differ on 133 455 of the 16.8 M colours (5° ties and truncation
/// edges), none of them in the spec's samples.
pub const PASTEL_HSV_FROM_BYTES: bool = true;
// spec-question: Q2 .NET 9+ byte conversion in the anti-alias pass
/// The staircase pass's byte conversions as Paint.NET on .NET 9+ runs them. common.md §5.4 infers that
/// .NET 9 saturates only the double → int32 step, and the int32 → byte step still keeps the low 8
/// bits, so these small values still wrap: the author's design the Outline file specifies. `Clamp`
/// is the Outline file's .NET 9+ reading (its Notes: five pixels of the radius-3 disc become 255).
pub const AA_BYTE_CONVERSION: ByteConv = ByteConv::Wrap;
// spec-question: Q22 Staircase colour copy at zero coverage
/// Whether staircase rule (a) or (d) gives a pixel N's colour even when the coverage it sets is 0
/// (true), or only when it gives the pixel some coverage (false: the literal "gives a transparent
/// pixel coverage", object-feather-object.md "Colour").
pub const AA_COPY_COLOUR_AT_ZERO: bool = false;

// spec-question: Q27 degenerate Square
/// render-gradient.md Notes: prescribed parameter when either integer half-extent is zero.
pub const GRADIENT_DEGENERATE_SQUARE_T: f64 = 0.0;

// spec-question: Q28 HSV gradient endpoint roundtrip
/// R110: retain prescribed f64 hexcone/truncation at unforced HSV endpoints (false),
/// or bypass that roundtrip to preserve exact From/To bytes (true), including radial centers.
/// The explicit radial d >= R exact-To bypass is unconditional and separate.
pub const GRADIENT_HSV_EXACT_ENDPOINTS: bool = false;

// spec-question: Q30 Inner Shadow object colour/alpha swizzle
/// Restore source alpha while preserving straight composite RGB (true), or premultiplied RGB (false).
pub const INNER_SHADOW_STRAIGHT_COLOR_SWIZZLE: bool = true;

// spec-question: Q5 Feather Selection r <= 1 mask grey
/// Use the CPU mask's grey byte directly as alpha (true), or decode it as sRGB first (false).
pub const FEATHER_SELECTION_MASK_GREY_AS_IS: bool = true;
// spec-question: Q31 Feather Selection zero-width widen
/// Width in pixels of the assumed band at radius zero; zero skips the stroke, positive radii use 2r.
pub const FEATHER_SELECTION_ZERO_WIDTH_BAND: f32 = 0.0;

// spec-question: Q32 Feather Gaussian kernel/support/border
/// R121: Feather-only point-sampled Gaussian with support ceil(6σ) (true), or the shared splatted HQ kernel (false).
/// Both are stand-ins for undocumented option 3; transparent off-canvas reads remain explicit at the call site.
pub const FEATHER_POINT_SAMPLED_GAUSSIAN: bool = true;

// spec-question: Q33 Feather Object erosion boundary
/// Assume off-canvas morphology samples are transparent, so objects also shrink from canvas edges.
pub const FEATHER_OBJECT_EROSION_EDGE: Edge = Edge::Transparent;

// spec-question: Q34 Hard-edge custom exponent order
/// object-bevel-object.md step 2: interpret custom options 2 then 3 as power then root.
pub const HARD_EDGE_POWER_FIRST: bool = true;
// spec-question: Q16 Emboss flat level
/// Bevel's relief is centred on exactly 0.5, unlike built-in Emboss's 128/255.
pub const RELIEF_FLAT: f32 = 0.5;

// spec-question: Q35 Bevel Selection default fill antialiasing
/// Device-default fill is assumed antialiased; false retains the aliased alternative.
pub const BEVEL_SELECTION_ANTIALIAS: bool = true;

// spec-question: Q36 Seam carving freed pixel RGB
/// Named Transparent is assumed white with zero alpha.
pub const SEAM_FREED_PIXEL: [u8; 4] = [255, 255, 255, 0];
// spec-question: Q37 Seam carving warning frequency
/// R108: accepted generations warn once, and OK reuses that warning.
pub const SEAM_NOTICE_ONCE_PER_GENERATION: bool = true;

// spec-question: Q38 Dither direct-helper empty current palette
/// R137: only an empty direct-helper palette falls back to the existing app defaults.
pub const DITHER_EMPTY_CURRENT_PALETTE: &[[u8; 4]] = &crate::effect::DEFAULT_PALETTE_PX;

// spec-question: Q56 Effect alpha hex exact width
/// R153: six ASCII digits retain alpha, eight set RRGGBBAA; wider all-hex input saturates on leave.
pub const EFFECT_ALPHA_HEX_EXACT_WIDTH: [usize; 2] = [6, 8];

// spec-question: Q75 Paste Alpha bilinear byte conversion
/// R167: f32 fractions truncate at 1/256; final alpha rounds, alpha-weighted RGB truncates.
/// Native grayZoom2/alphaZoom2/grayZoom3 fixtures discriminate these byte conversions.
/// The caller retains the specified, unmeasured zero-sum-alpha handling.
pub const PASTE_ALPHA_BILINEAR_BYTE_POLICY: fn([u64; 3], u64) -> crate::pixel::Px = |rgb, alpha| {
    [
        (rgb[0] / alpha) as u8,
        (rgb[1] / alpha) as u8,
        (rgb[2] / alpha) as u8,
        ((alpha + 32768) >> 16) as u8,
    ]
};

// spec-question: Q64 From File typed browser paths
/// R156 FILE_BROWSER_TYPED_PATH: a browser cannot open a typed path; only an attached pick loads.
pub const FILE_BROWSER_TYPED_PATH: &str = "Use Browse to choose a file in this browser.";

/// common.md §5.10, R47: the document face for the specs' Arial (and Microsoft Sans Serif,
/// Segoe UI), the bundled interface face at the fixed 96 DPI.
pub const SANS_FONT: &str = "Selawik";
// spec-question: Q51 Horizon indicator glyph fallback
/// R152 HORIZON_INDICATOR_CARLITO: Selawik has no ↔/↕, so Level Horizon's leading indicator
/// (and, by Q77, U+2212) is drawn in this bundled face, aligned with the rest of the label by
/// common ascent.
pub const HORIZON_INDICATOR_FONT: &str = "Carlito";
// spec-question: Q53 Horizon raw angle and float operation order
/// R152 HORIZON_SWAPPED_ATAN2: Level Horizon's (rotation, plumb) from a non-zero f32 handle delta,
/// in exactly this f32 operation order; the rotation is never normalized.
pub const HORIZON_SWAPPED_ATAN2: fn(f32, f32) -> (f32, bool) = |dx, dy| {
    let raw = (dx + 0.0).atan2(dy + 0.0).to_degrees() - 90.0;
    let plumb = (raw <= -45.0 || raw >= 45.0) && !(raw > -225.0 && raw < -135.0);
    ((if plumb { raw + 90.0 } else { raw }) + 0.0, plumb)
};
// spec-question: Q76 Horizon guideline dash caps
/// R174 HORIZON_DASH_CAPS: every dash of the guideline is round-capped (true), or the dashes are
/// flat and only the line's two ends get round caps where a dash meets them (false).
pub const HORIZON_DASH_ROUND_CAPS: bool = true;
// spec-question: Q77 Horizon label characters the bundled faces lack
/// R175 HORIZON_LABEL_GLYPH_ROUTE: each scalar of the formatted Horizon label → the scalar drawn
/// and its face, or `None` for a zero-advance invisible format control. The indicators and
/// U+2212 use Carlito (Q51's route); U+066B, which no bundled face has, is drawn as a comma.
pub const HORIZON_LABEL_GLYPH_ROUTE: fn(char) -> Option<(char, &'static str)> = |c| match c {
    '↔' | '↕' | '\u{2212}' => Some((c, HORIZON_INDICATOR_FONT)),
    '\u{61c}' | '\u{200e}' | '\u{200f}' => None,
    '\u{66b}' => Some((',', SANS_FONT)),
    c => Some((c, SANS_FONT)),
};
// spec-question: Q52 Dimensions bitmap-only labels
/// R152 DIMENSIONS_OUTLINE_PRESENTATION: the rendering projection of a Dimensions label.
pub const DIMENSIONS_OUTLINE_PRESENTATION: fn(&str) -> String = super::text::outline_presentation;
// spec-question: Q55 Dimensions halo antialiasing
/// R152 DIMENSIONS_HALO_STROKE_ALPHA: a premultiplied drawing-layer channel under halo
/// coverage h (GDI+ copy mode with a transparent colour): every channel × (1 − h), rounded.
pub const DIMENSIONS_HALO_STROKE_ALPHA: fn(u8, f32) -> u8 =
    |c, h| (c as f32 * (1.0 - h)).round() as u8;

/// R70 Decision 2: Meme Maker's face, from the system font collection (native System policy).
pub const MEME_FONT: &str = "Impact";
/// R70 Decision 2, R157: the bundled face a session uses once Impact is absent or unusable, and
/// always under the Bundled policy and on the web. Registered under its own name, never Impact.
pub const IMPACT_FALLBACK: &str = "Anton";
// spec-question: Q14 Meme Maker "All caps" culture
/// R157 MEME_CURRENT_CULTURE_SIMPLE_UPPER: All caps maps each scalar through the captured
/// culture's public .NET 10.0.11 / ICU 78.3 simple-uppercase snapshot (`meme_casing.json`),
/// looked up by exact name, parents, then English; only the explicit invariant name "" is
/// invariant. A Linux runtime snapshot, not Windows NLS or future-Unicode equivalence.
pub const MEME_CURRENT_CULTURE_SIMPLE_UPPER: fn(&str, &str) -> String = super::meme::upper;
// spec-question: Q66 Meme outline presentation
/// R157 MEME_OUTLINE_PRESENTATION: the private rendering projection of a caption line, VS15 on
/// every scalar (Dimensions' utility), measured and drawn alike; stored text keeps its selectors.
pub const MEME_OUTLINE_PRESENTATION: fn(&str) -> String = super::text::outline_presentation;
// spec-question: Q67 Meme word-wrap boundaries
/// R157 MEME_WORD_WRAP: one paragraph's visual lines, greedy over indivisible units (a word with
/// its trailing separators; leading separators form a unit of their own). Each line takes the
/// most units whose measured width is <= the box (trailing separators per
/// `MEME_FIT_TRAILING_SEPARATORS`); a first unit that alone overflows stays whole, never split.
pub const MEME_WORD_WRAP: MemeWrap = super::meme::wrap_paragraph;
/// A paragraph, the box width and a candidate's measured width → the paragraph's visual lines.
pub type MemeWrap = for<'a> fn(&'a str, f64, &dyn Fn(&str) -> f64) -> Vec<&'a str>;
// spec-question: Q67 Meme word-wrap boundaries
/// R157: a break opportunity follows Unicode whitespace other than U+00A0/U+202F, and U+200B.
pub const MEME_BREAK_AFTER: fn(char) -> bool =
    |c| c == '\u{200b}' || (c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{202f}'));
// spec-question: Q67 Meme word-wrap boundaries
/// R157: a line's trailing separators count toward its fit (true); false lets them hang past the
/// box as DirectWrite's trailing whitespace does.
pub const MEME_FIT_TRAILING_SEPARATORS: bool = true;
// spec-question: Q67 Meme word-wrap boundaries
/// R157: a centred line is centred by its measured advance with its trailing separators (true);
/// false centres it without them, as DirectWrite aligns.
pub const MEME_ALIGN_TRAILING_SEPARATORS: bool = true;

// spec-question: Q58 Calendar culture projection
/// R154 CALENDAR_GREGORIAN_CULTURE_SNAPSHOT: the captured culture's Gregorian month and weekday
/// names and numeric short-date templates (`assets/lang/date-formats.json`: public .NET 10 / ICU
/// 78.3 DateTimeFormatInfo with its only optional Gregorian calendar assigned, user overrides
/// off), by exact name, parents, then English. One profile gives the title, the month list, the
/// weekday row and the date parser; default Persian/Thai calendars are projected to Gregorian.
pub const CALENDAR_GREGORIAN_CULTURE_SNAPSHOT: fn(&str) -> super::calendar::DateProfile =
    super::calendar::date_profile;
// spec-question: Q59 Calendar date parser
/// R154 CALENDAR_DATE_PARSE_SUBSET: a holiday's date part in a culture → its date. Whole-input
/// ISO yyyy-MM-dd first (an invalid one is never reinterpreted), then the profile's numeric
/// templates (distinct dates from several matches are rejected), then `day month-name year` or
/// `month-name day year` by Unicode-lowercase alias equality; years 1..9999 and real Gregorian
/// days only. A bounded subset of .NET TryParse: no times, eras, missing fields or native digits.
pub const CALENDAR_DATE_PARSE_SUBSET: fn(&str, &str) -> Option<crate::effect::rng::CivilDate> =
    super::calendar::parse_date;
// spec-question: Q60 Calendar endpoint years
/// R154 CALENDAR_BOUNDARY_YEARS: the dialog's year slider for the captured UTC year Y,
/// [max(1, Y − 120), min(9999, Y + 120)]. Selected years are 1..9999; the 42 cells extend
/// proleptically into years 0 and 10000, where the parser admits no holiday.
pub const CALENDAR_BOUNDARY_YEARS: fn(i32) -> [i32; 2] = |y| {
    let y = i64::from(y.clamp(1, 9999));
    [(y - 120).max(1) as i32, (y + 120).min(9999) as i32]
};
// spec-question: Q61 Calendar font order and raster
/// R154 CALENDAR_FONT_ORDER: the font drop-downs' (family, display name) entries, sorted by
/// Unicode-lowercase display name, then display name, then family (no locale collation).
pub const CALENDAR_FONT_ORDER: fn(&mut [(String, String)]) =
    |v| v.sort_by(|a, b| (a.1.to_lowercase(), &a.1, &a.0).cmp(&(b.1.to_lowercase(), &b.1, &b.0)));
// spec-question: Q61 Calendar font order and raster
/// R154: the main font draws bold (its 700 face, else R111 synthesis) exactly when the family's
/// own face weighs 400; any other weight is drawn as it is. Both fonts use the shared Smooth
/// antialiased fractional-metric renderer, the stand-in for DirectWrite's natural mode.
pub const CALENDAR_BOLD_WHEN_NORMAL: fn(&str) -> bool = super::calendar::bold_when_normal;
// spec-question: Q62 Calendar wrapping
/// R154 CALENDAR_DOCUMENT_WRAP: one paragraph's visual lines within the document width. A line
/// ends after the latest Unicode whitespace or U+200B that fits (the whitespace stays on it;
/// `CALENDAR_FIT_TRAILING_WHITESPACE` says whether it counts), else after the most
/// scalar-plus-selector units that fit, else after one unit (also at width 0). `advances` gives
/// each scalar's advance laid out from a start.
pub const CALENDAR_DOCUMENT_WRAP: CalendarWrap = super::calendar::wrap_paragraph;
/// A paragraph, the width and a run's per-scalar advances → the paragraph's visual lines.
pub type CalendarWrap = for<'a> fn(&'a str, f64, &dyn Fn(&str) -> Vec<f64>) -> Vec<&'a str>;
// spec-question: Q62 Calendar wrapping
/// R154: a candidate line's trailing whitespace counts toward its fit (true); false lets it hang
/// past the width as DirectWrite's trailing whitespace does.
pub const CALENDAR_FIT_TRAILING_WHITESPACE: bool = true;
// spec-question: Q63 Calendar faint color glyphs
/// R154: a faint run's alpha 64 applies once to the whole run, colour bitmap glyphs included
/// with their own RGB (true); false keeps bitmap glyphs opaque under the faint outlines.
pub const CALENDAR_FAINT_COLOR_GLYPHS: bool = true;
// spec-question: Q68 Text Fun Factory raw units and whole prefixes
/// R158 TEXT_FUN_RAW_UTF16_UNITS: (units, family, px, bold, italic) → for each raw UTF-16 unit k,
/// the width of the whole raw prefix `units[..k]` measured as one line (trailing spaces in; a pair
/// inside it decodes, a half at its end is U+FFFD; not necessarily additive), and the line drawn
/// as its outline (the unit alone, VS15-projected so a colour face never draws it; none for
/// U+0020), all with the same faces.
pub const TEXT_FUN_RAW_UTF16_UNITS: fn(&[u16], &str, f32, bool, bool) -> RawUnits =
    super::text_fun::raw_units;
/// Each unit's prefix width and drawn line.
pub type RawUnits = (Vec<f64>, Vec<Option<crate::tool::text::TextLine>>);
// spec-question: Q69 Text Fun Factory unavailable exact Bold700 face
/// R158 TEXT_FUN_BOLD_FACE_FALLBACK: the face slot Bold / Italics request (Bold = the family's
/// 700 face). Unchecked keeps the selected face's own bytes and metadata; with no Bold face Text
/// keeps the selected bytes and thickens only a face of weight <= 600 (R111), so Bold on a lone
/// 900 face stays 900 (native: 700). No synthetic thinning, no fake metadata.
pub const TEXT_FUN_BOLD_FACE_FALLBACK: fn(bool, bool) -> crate::fonts::FaceStyle =
    |bold, italic| {
        use crate::fonts::FaceStyle;
        match (bold, italic) {
            (true, true) => FaceStyle::BoldItalic,
            (true, false) => FaceStyle::Bold,
            (false, true) => FaceStyle::Italic,
            (false, false) => FaceStyle::Regular,
        }
    };
// spec-question: Q82 Text Fun Factory rotated corner in Expected results 5
/// R187 TEXT_FUN_ROTATION_CLOCKWISE: a positive Rotate angle turns a letter clockwise on screen
/// ("Letter transforms"; its top-right corner moves right and down). False turns it the other way.
pub const TEXT_FUN_ROTATION_CLOCKWISE: bool = true;
// spec-question: Q70 Text Fun Factory gradient raw-working transfers
/// R158 TEXT_FUN_GRADIENT_RAW_WORKING: the straight working RGB of a gradient at t (clamped
/// 0…1) between stop bytes u0 and u1, taken raw (byte/255): checked "sRGB gradient" lerps them,
/// unchecked lerps their sRGB decodes and encodes the result. Either way the value is used as
/// a working value (never decoded again), so the Linear output boundary encodes it once more;
/// t = 0 / 1 are the exact raw stops.
pub const TEXT_FUN_GRADIENT_RAW_WORKING: fn([u8; 3], [u8; 3], f32, bool) -> [f32; 3] =
    super::text_fun::gradient_raw_working;
// spec-question: Q71 Text metal height power
/// R159 TEXT_FUN_METAL_MASK_POWER: Gold and Chrome raise the height mask's RGB to this power
/// before their Bokeh, with no inverse after it; it changes only fractional (antialiased)
/// coverage. 1/2.2 is the unobserved alternative.
pub const TEXT_FUN_METAL_MASK_POWER: f32 = 2.2;
// spec-question: Q72 Text Bokeh defaults
/// R159 TEXT_FUN_BOKEH_GAMMA_BOOST: Text Fun Factory's Bokeh blurs (quality 4 hard bevel,
/// quality 10 Gold/Chrome) keep Paint.NET's canonical default gamma boost.
pub const TEXT_FUN_BOKEH_GAMMA_BOOST: f64 = 0.0;
// spec-question: Q72 Text Bokeh defaults
/// R159 TEXT_FUN_BOKEH_EDGE: and its canonical default edge mode (not BB15's Transparent).
pub const TEXT_FUN_BOKEH_EDGE: Edge = Edge::Mirror;
// spec-question: Q73 Text Gaussian high-quality interpretation
/// R159 TEXT_FUN_GAUSSIAN_SHARED_HQ: the soft bevel's "highest quality" Gaussian is the shared
/// splatted HQ kernel (`blocks::blur_soft`, σ = D·RADIUS_TO_SIGMA, support ⌈6σ⌉). It is the only
/// kernel implemented; an evidence-calibrated one would replace it at `text_fun::bevel_height`.
pub const TEXT_FUN_GAUSSIAN_SHARED_HQ: bool = true;
// spec-question: Q74 Text metal relief range
/// R159 TEXT_FUN_METAL_CLAMP_RELIEF: Gold and Chrome blend the raw, unclamped Emboss relief E
/// (false); true clamps E to 0…1 first (Gold's blue then turns 1 for every E ≥ 1, not only E = 1).
pub const TEXT_FUN_METAL_CLAMP_RELIEF: bool = false;
// spec-question: Q43 Chart finite-data and resource policies
/// R145: the estimated peak of new chart and adapter storage a render may need, checked before
/// any allocation: 5P plus the geometry allowance (P = selection-bounds pixels), and for partial
/// bounds at least 4C + 4P (C = document pixels). An estimate, not an allocator sandbox: document,
/// session and GPU memory lie outside it.
pub const CHART_NEW_ALLOCATION_BUDGET_BYTES: u64 = 256 * 1024 * 1024;
// spec-question: Q43 Chart finite-data and resource policies
/// R145: the transient geometry/text scratch counted inside that budget; never reserved.
pub const CHART_GEOMETRY_ALLOWANCE_BYTES: u64 = 32 * 1024 * 1024;
// spec-question: Q43 Chart finite-data and resource policies
/// R145: the paint.rs chart format's limits, not native ones: encoded JSON bytes, series, points
/// per series, points, UTF-16 units per string, aggregate UTF-8 text, value-axis intervals. The
/// typed JSON schema nests 5 levels, inside R145's 8.
pub const CHART_MAX_ENCODED_BYTES: usize = 8 * 1024 * 1024;
pub const CHART_MAX_SERIES: usize = 64;
pub const CHART_MAX_POINTS_PER_SERIES: usize = 4096;
pub const CHART_MAX_POINTS: usize = 16384;
pub const CHART_MAX_STRING_UTF16: usize = 32767;
pub const CHART_MAX_TEXT_BYTES: usize = 1024 * 1024;
pub const CHART_MAX_INTERVALS: u32 = 10;
// spec-question: Q43 Chart finite-data and resource policies
/// R145 CHART_FINITE_RANGE_FALLBACK: an axis range `[lo, hi]` (lo ≤ hi) that nice steps or
/// category padding cannot represent, as a finite range of positive width (itself, or its
/// neighbouring floats); its ticks are its two ends. Distinct extreme values can share a pixel.
pub const CHART_FINITE_RANGE_FALLBACK: fn(f64, f64) -> (f64, f64) =
    super::chart::axes::finite_range;
// spec-question: Q43 Chart finite-data and resource policies
/// R145: a bubble's diameter from (its second value, the largest positive second value of the
/// rendered Bubble series or 0, the plot's smaller side): the specified 9 % when no second value
/// is positive; otherwise 9 % × √(value2 / largest), and no disk for a nonpositive one.
pub const CHART_BUBBLE_DIAMETER: fn(f64, f64, f64) -> Option<f64> = |v2, largest, side| {
    if largest <= 0.0 {
        Some(0.09 * side)
    } else {
        (v2 > 0.0).then(|| 0.09 * side * (v2 / largest).sqrt())
    }
};
// spec-question: Q43 Chart finite-data and resource policies
/// R145: the most straight pieces all of a chart's 3-D splines are cut into (ribbons, or both
/// edges of slabs): each span gets at most this ÷ (2 × the chart's points).
pub const CHART_MAX_EXPANDED_VERTICES: usize = 524288;
// spec-question: Q44 Chart named-color list and fallback consumption
/// R146 CHART_PALETTE_UNSET_ONLY: an unset series colour takes the next `DEFAULT_PALETTE_15`
/// entry, counted over unset colours only from 0 each render (true), or the entry at the series'
/// rendered position (false). Stored colours never change.
pub const CHART_PALETTE_UNSET_ONLY: bool = true;
// spec-question: Q45 Mixed chart families
/// R149 CHART_MIXED_FAMILY_OVERLAY: whether stored series `index` of a type is drawn: every
/// non-pie-like series, a pie-like one only at index 0. All share one plot in stored order; the
/// first drawn series' family (Cartesian, or transposed Bar) draws the axes and titles.
pub const CHART_MIXED_FAMILY_OVERLAY: fn(usize, super::chart::model::ChartType) -> bool =
    |index, t| index == 0 || !t.is_pie_like();
// spec-question: Q46 Ragged chart categories and editor anchors
/// R149 CHART_RAGGED_CATEGORY_LAYOUT: each x's category label lines, the distinct nonempty names
/// in series then point order; different names at one x stack, identical ones share a line.
pub const CHART_RAGGED_CATEGORY_LAYOUT: super::chart::axes::CategoryLabels =
    super::chart::axes::category_labels;
// spec-question: Q46 Ragged chart categories and editor anchors
/// R149: the category band (Column/Bar slots, axis margins) from the sorted distinct x.
pub const CHART_CATEGORY_WIDTH: fn(&[f64]) -> f64 = super::chart::axes::category_width;
// spec-question: Q49 Chart automatic layout constants
/// R149 CHART_MEASURED_LAYOUT_V1: provisional port margins in px, around measured text: outer
/// inset, gap between regions and texts, legend symbol width, tick length.
pub const CHART_MEASURED_LAYOUT_V1: super::chart::ChartLayout = super::chart::ChartLayout {
    inset: 8.0,
    gap: 4.0,
    legend_symbol: 12.0,
    tick: 4.0,
};
// spec-question: Q92 Chart series clipping and area fill rule
/// R189 CHART_SERIES_CLIP_AND_FILL: series are clipped to the plot rectangle, cutting markers and
/// bubbles on its edges (`clip_to_plot`; false: only the chart's edges clip them), and areas fill
/// with GDI+'s default alternate rule (`EvenOdd`; `Winding` is nonzero).
pub const CHART_SERIES_CLIP_AND_FILL: super::chart::SeriesClipAndFill =
    super::chart::SeriesClipAndFill {
        clip_to_plot: true,
        area_fill: tiny_skia::FillRule::EvenOdd,
    };
// spec-question: Q80 Chart text drawing
/// The private drawing projection of chart text (GDI+ draws outlines, never colour bitmaps): CRLF
/// and CR are line breaks like LF, and every scalar requests text presentation (Dimensions'
/// VS15 utility). Stored strings are unchanged.
pub const CHART_TEXT_PROJECTION: fn(&str) -> String = super::chart::text_projection;
// spec-question: Q90 Chart value-axis labels
/// Value-axis labels as the chart library formats doubles: .NET `Double.ToString()` (General)
/// in the invocation's captured culture.
pub const CHART_VALUE_LABEL_FORMAT: fn(f64, &str) -> String = super::chart::axes::format_general;
// spec-question: Q91 Chart axis and legend drawing details
/// The value axis takes the most 1/2/5 intervals (≤ `CHART_MAX_INTERVALS`) whose labels plus a
/// gap fit each interval (`fit_labels`; false: always up to 10). Category ticks and grid lines sit
/// at every distinct x. Legend boxes are `legend_box_height` px tall, line symbols the 1 px series
/// pen, point symbols the 5 px marker. Axis lines and ticks draw over the series
/// (`axes_over_series`), grid lines under them.
pub const CHART_AXIS_DRAWING_V1: super::chart::AxisDrawing = super::chart::AxisDrawing {
    fit_labels: true,
    legend_box_height: 8.0,
    axes_over_series: true,
};
// spec-question: Q48 Negative radial and proportional chart values
/// R149 CHART_NONPOSITIVE_RADIAL_EMPTY: the drawn Radar/Polar radius or Funnel/Pyramid height of a
/// value, nonpositive values drawing at the centre or with no height; stored values never change.
/// Pie keeps the specified absolute values and Bubble its own size rule.
pub const CHART_NONPOSITIVE_RADIAL_EMPTY: fn(f64) -> f64 = |v| v.max(0.0);
// spec-question: Q50 Chart 3D orientation and thickness
/// R194 (revising R149) CHART_3D_OBLIQUE_RIGHT_ANGLE_V1: "an oblique 3-D box … right-angle axes":
/// model x right, y up, z depth, projected obliquely to (x + s z, −y − s c z) with c = cos 30°,
/// s = sin 30° (the depth vector of a 30° elevation and 30° rotation; the front face stays an
/// axis-aligned rectangle; no perspective) and fitted uniformly to the plot. Each axis-type
/// series is its category slot (the 2-D point width) deep, followed by an equal gap, packed from
/// the front with the last series nearest and drawn last. Planar faces shade by their normal's
/// largest component: up ×`top`, along x ×`side`, else ×1; curved rims, a Doughnut's inner wall
/// and a Funnel's cones are sides; channels are clamped. Pie and Doughnut lie in x–z, `disc_thickness` of the diameter
/// thick, its tilt the same oblique depth mapping; Funnel sections are round and Pyramid sections
/// square; point markers stay circles on screen; Radar and Polar stay flat.
pub const CHART_3D_OBLIQUE_RIGHT_ANGLE_V1: super::chart::three_d::Oblique =
    super::chart::three_d::Oblique {
        project: super::chart::three_d::project,
        top: 1.2,
        side: 0.8,
        disc_thickness: 0.1,
    };
// spec-question: Q93 Chart pie labels and pie-like areas
/// A Pie label's centre on its slice's bisector, in radii from the (pushed) slice's centre;
/// Doughnut labels sit mid-ring. Labels fit when their boxes lie inside the chart area; when none
/// of the 19 sizes fits, the smallest is drawn, labels clipped. Funnel and Pyramid fill the chart
/// area, their segments touching.
pub const CHART_PIE_LABEL_RADIUS: f64 = 0.5;
// spec-question: Q94 Chart Radar and Polar decorations
/// Polar's spokes, every this many degrees from the top, labelled in degrees like the value
/// axis. Radar's spokes are its categories, labelled with their names. Both circles take the
/// area that their spoke labels, set just outside the rim, leave; rings sit at value ticks
/// fitted like Q91 with their labels left of the top spoke; rings and spokes lie under the
/// series; no axis titles. Legend symbols: Polar a line, Radar a box (Q91's line and area
/// symbols).
pub const CHART_POLAR_SPOKE_DEGREES: f64 = 30.0;
// spec-question: Q95 Chart 3-D walls, grid and axis labels
/// The 3-D back wall and floor colour, shaded like series faces, under the series. The 2-D grid's
/// rows run across the back wall, its columns up the back wall and across the floor; the axis
/// lines run along the back wall's left edge and the floor's front edge (no ticks). The left
/// axis's labels sit level with their back-wall rows, left of the whole box; the bottom axis's
/// under the floor's front edge.
pub const CHART_3D_WALL_COLOUR: u32 = 0xFFFFFF;
// spec-question: Q42 Chart color picker HSV operation order
/// R144 CHART_PICKER_HSV_RADIANS_EXPANDED_T: the picker keeps hue in radians, computed from byte
/// channels with the numerator scaled before the division, and converts back with the expanded
/// rising channel t = V·((1 − S) + S·f) (true). False keeps hue in degrees and uses the ordinary
/// t = V·(1 − S·(1 − f)). Channels are then ×255, clamped and truncated.
pub const CHART_PICKER_HSV_RADIANS_EXPANDED_T: bool = true;
// spec-question: Q44 Chart named-color list and fallback consumption
/// R146 CHART_RANDOM_NAMED_COLORS_LEGACY140: a new row's or an unremembered slice's random colour
/// is one of these 140 names (KnownColor 28…167, AliceBlue…YellowGreen; Aqua/Cyan and
/// Fuchsia/Magenta stay separate names), drawn uniformly by unbiased rejection sampling.
pub const CHART_RANDOM_NAMED_COLORS_LEGACY140: &[(&str, u32); 140] =
    &super::chart::edit::NAMED_COLORS;
// spec-question: Q47 Chart mixed-cell sorting and locale parsing
/// R149 CHART_GRID_COMPARE_INSERTION: a grid column sorts by stable insertion of the rows'
/// cached keys, numerically when both cells parse in the captured culture, else by ordinal
/// UTF-16 text (the hybrid comparison is not a total order, so no comparator sort is used).
pub const CHART_GRID_COMPARE_INSERTION: fn(&mut [usize], &[super::chart::edit::CellKey], bool) =
    super::chart::edit::insertion_sort;
// spec-question: Q98 Chart unreadable remembered settings
/// The chart a remembered token that does not decode opens the dialog on, and so stores on OK:
/// the default chart. (Rendering such a token without the dialog leaves the source unchanged,
/// R143.)
pub const CHART_UNREADABLE_TOKEN: fn() -> super::chart::model::Chart =
    <super::chart::model::Chart as Default>::default;
