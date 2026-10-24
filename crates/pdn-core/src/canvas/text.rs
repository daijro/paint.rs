//! Port target: Canvas 2D text (`font`, `textAlign`, `textBaseline`, `fillText`, `measureText`)
//! as used by js/src/engine/document/tool/tool/TextTool.js and js/src/engine/AppView.js.
//! Reference: paintdotjs @ b4e6fc5
//!
//! Simple LTR shaping: cmap glyphs, hmtx advances, and pair kerning: GPOS `kern` pair
//! adjustments like HarfBuzz in Chromium (pinned by canvas-measure-text: "Hello, World" is
//! 0.2px narrower with them), or the `kern` table in fonts without GPOS.
//!
//! Rendering follows Chromium on Linux: glyph origins on quarter pixels and whole-pixel
//! baselines, a vertical approximation of FreeType light autohinting, exact-area glyph coverage,
//! and Skia's mask gamma (contrast 0.2, gamma 1.2).
use super::Path2D;
use std::collections::HashMap;
use std::sync::{
    Arc, LazyLock, RwLock,
    atomic::{AtomicU64, Ordering},
};
static FONT_GENERATION: AtomicU64 = AtomicU64::new(0);
/// Changes only after registered font bytes become visible (including removal).
pub fn font_generation() -> u64 {
    FONT_GENERATION.load(Ordering::Acquire)
}
use ttf_parser::gpos::{PairAdjustment, PositioningSubtable};
use ttf_parser::{Face, GlyphId, OutlineBuilder, Tag};

/// paintdotjs's webfonts (css/theme.css), embedded once for the canvas and the UI.
pub static NOTO_SANS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/font/noto-sans/NotoSans-Regular.ttf"
));
pub static NOTO_SANS_MONO: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/font/noto-sans-mono/NotoSansMono-Regular.ttf"
));
/// R131's RTL faces: UI fallbacks, and Text's after Noto Sans (R183 follow-up).
pub static NOTO_SANS_ARABIC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/font/noto-sans-arabic/NotoSansArabic-Regular.ttf"
));
pub static NOTO_SANS_HEBREW: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/font/noto-sans-hebrew/NotoSansHebrew-Regular.ttf"
));

/// Font bytes: embedded faces are borrowed, registered ones shared.
#[derive(Clone)]
enum FontBytes {
    Embedded(&'static [u8]),
    Registered(Arc<[u8]>),
}

impl std::ops::Deref for FontBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            FontBytes::Embedded(b) => b,
            FontBytes::Registered(b) => b,
        }
    }
}

/// Process-wide registry, keyed by ASCII-lowercased family (CSS family names are
/// case-insensitive). The embedded webfonts and the UI/Calibri stand-ins (spec §0.5) are always
/// present.
static FONTS: LazyLock<RwLock<HashMap<(String, crate::fonts::FaceStyle), FontBytes>>> =
    LazyLock::new(|| {
        use crate::fonts::{CARLITO_REGULAR, FaceStyle, SELAWIK_REGULAR};
        RwLock::new(HashMap::from([
            (
                ("noto sans".to_string(), FaceStyle::Regular),
                FontBytes::Embedded(NOTO_SANS),
            ),
            (
                ("noto sans mono".to_string(), FaceStyle::Regular),
                FontBytes::Embedded(NOTO_SANS_MONO),
            ),
            (
                ("selawik".to_string(), FaceStyle::Regular),
                FontBytes::Embedded(SELAWIK_REGULAR),
            ),
            (
                ("carlito".to_string(), FaceStyle::Regular),
                FontBytes::Embedded(CARLITO_REGULAR),
            ),
        ]))
    });

/// Registers a font face for `family`. Bytes ttf-parser cannot parse are ignored.
pub fn register_font(family: &str, bytes: Vec<u8>) {
    register_font_style(family, crate::fonts::FaceStyle::Regular, bytes);
}

pub fn register_font_style(family: &str, style: crate::fonts::FaceStyle, bytes: Vec<u8>) {
    if Face::parse(&bytes, 0).is_ok() {
        FONTS.write().unwrap().insert(
            (family.to_ascii_lowercase(), style),
            FontBytes::Registered(bytes.into()),
        );
        FONT_GENERATION.fetch_add(1, Ordering::Release);
    }
}

pub fn has_font(family: &str) -> bool {
    FONTS.read().unwrap().contains_key(&(
        family.to_ascii_lowercase(),
        crate::fonts::FaceStyle::Regular,
    ))
}

/// Task35 document-font lookup; unlike the Canvas shim, absence does not choose a fallback.
pub fn registered_font_bytes(family: &str) -> Option<Arc<[u8]>> {
    FONTS
        .read()
        .unwrap()
        .get(&(
            crate::fonts::resolve_family(family).to_ascii_lowercase(),
            crate::fonts::FaceStyle::Regular,
        ))
        .map(|b| match b {
            FontBytes::Embedded(b) => Arc::from(*b),
            FontBytes::Registered(b) => b.clone(),
        })
}

/// Exact document family/style lookup; a regular face permits the existing style synthesis.
pub fn registered_style_font_bytes(
    family: &str,
    style: crate::fonts::FaceStyle,
) -> Option<Arc<[u8]>> {
    let fonts = FONTS.read().unwrap();
    let family = family.to_ascii_lowercase();
    fonts
        .get(&(family.clone(), style))
        .or_else(|| fonts.get(&(family, crate::fonts::FaceStyle::Regular)))
        .map(|b| match b {
            FontBytes::Embedded(b) => Arc::from(*b),
            FontBytes::Registered(b) => b.clone(),
        })
}
pub fn unregister_font(family: &str) {
    let mut fonts = FONTS.write().unwrap();
    let before = fonts.len();
    fonts.retain(|(name, _), bytes| {
        name != &family.to_ascii_lowercase() || matches!(bytes, FontBytes::Embedded(_))
    });
    if fonts.len() != before {
        FONT_GENERATION.fetch_add(1, Ordering::Release);
    }
}

/// Families resolve through `fonts::resolve_family` first (generics → Noto faces, "Segoe UI" →
/// Selawik, "Calibri" → Carlito), so layout never depends on host fonts; unregistered families
/// fall back to Noto Sans, as the paintdotjs font stacks end in it or in `sans-serif`.
fn font_bytes(family: &str) -> FontBytes {
    let fonts = FONTS.read().unwrap();
    fonts
        .get(&(
            crate::fonts::resolve_family(family).to_ascii_lowercase(),
            crate::fonts::FaceStyle::Regular,
        ))
        .unwrap_or_else(|| &fonts[&("noto sans".to_owned(), crate::fonts::FaceStyle::Regular)])
        .clone()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub family: String,
    pub size_px: f64,
    pub bold: bool,
    pub italic: bool,
}

impl Default for Font {
    /// Canvas default `10px sans-serif`.
    fn default() -> Self {
        Self {
            family: "sans-serif".into(),
            size_px: 10.0,
            bold: false,
            italic: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Start,
    End,
    Left,
    Right,
    Center,
}

impl TextAlign {
    pub fn from_css(s: &str) -> Self {
        match s {
            "end" => TextAlign::End,
            "left" => TextAlign::Left,
            "right" => TextAlign::Right,
            "center" => TextAlign::Center,
            _ => TextAlign::Start,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextBaseline {
    Top,
    Hanging,
    Middle,
    #[default]
    Alphabetic,
    Ideographic,
    Bottom,
}

impl TextBaseline {
    pub fn from_css(s: &str) -> Self {
        match s {
            "top" => TextBaseline::Top,
            "hanging" => TextBaseline::Hanging,
            "middle" => TextBaseline::Middle,
            "ideographic" => TextBaseline::Ideographic,
            "bottom" => TextBaseline::Bottom,
            _ => TextBaseline::Alphabetic,
        }
    }
}

/// `measureText()` results. The ink box is measured from the alignment point and the baseline,
/// like Chromium (TextTool.measureTextLayout reads all four sides).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    pub width: f64,
    pub actual_bounding_box_left: f64,
    pub actual_bounding_box_right: f64,
    pub actual_bounding_box_ascent: f64,
    pub actual_bounding_box_descent: f64,
}

/// Skia fake italic (Blink `SetupSkFont`: `setSkewX(-1/4)`) for regular-only faces.
const FAKE_ITALIC_SKEW: f64 = -0.25;

/// Skia's stroke fake bold (`SkScalerContext` with `kEmbolden`): a stroke of width
/// `size · lerp(1/24 at 9px, 1/32 at 36px)` (`kStdFakeBoldInterpKeys/Values`) filled together
/// with the outline, i.e. the outline grows by half that width on every side.
fn fake_bold_width(size: f64) -> f64 {
    let k = 1.0 / 24.0 + (1.0 / 32.0 - 1.0 / 24.0) * ((size - 9.0) / 27.0).clamp(0.0, 1.0);
    size * k
}

/// Per-glyph outlines in user space, aligned and baseline-adjusted from the requested (x, y).
pub(super) struct Layout {
    pub glyphs: Vec<Path2D>,
    pub metrics: TextMetrics,
}

/// Rounds to Blink's LayoutUnit (1/64 px, `FromFloatRound`).
fn layout_unit(v: f64) -> f64 {
    (v * 64.0).round() / 64.0
}

/// Offset from the requested baseline to the alphabetic baseline (positive = down), matching
/// Blink `GetFontBaseline` (values probed in Chromium for Noto Sans at 10/13/14/20/37px):
/// top/bottom/middle use OS/2 typo metrics normalized to the em, hanging is 4/5 of the rounded
/// ascent, ideographic the rounded descent.
fn baseline_shift(face: &Face, size: f64, baseline: TextBaseline) -> f64 {
    let upem = face.units_per_em() as f64;
    let ascent = (face.ascender() as f64 * size / upem).round();
    let descent = (-(face.descender() as f64) * size / upem).round();
    let (ta, td) = match (face.typographic_ascender(), face.typographic_descender()) {
        (Some(a), Some(d)) if a as i32 - (d as i32) > 0 => (a as f64, -(d as f64)),
        _ => (ascent, descent),
    };
    let em_ascent = layout_unit(ta * size / (ta + td));
    let em_descent = layout_unit(size) - em_ascent;
    match baseline {
        TextBaseline::Top => em_ascent,
        TextBaseline::Hanging => ascent * 4.0 / 5.0,
        TextBaseline::Middle => (em_ascent - em_descent) / 2.0,
        TextBaseline::Alphabetic => 0.0,
        TextBaseline::Ideographic => -descent,
        TextBaseline::Bottom => -em_descent,
    }
}

/// FreeType light autohinting, which Chromium on Linux renders text with, approximated
/// vertically (it does not touch x): the x-height overshoot is rounded up from 3/8 px
/// (`af_latin_metrics_scale_dim`), which rescales the font vertically; blue zones then snap to
/// whole pixels, overshoots below half a pixel vanish, and the outline is interpolated between
/// the zones. Pinned by the text-noto-* cases (text-noto-14 mean diff 8.08 → 6.38).
struct Hinter {
    /// (font units, pixels above the baseline), ascending.
    knots: Vec<(f64, f64)>,
    scale: f64,
}

impl Hinter {
    fn new(face: &Face, ppem: f64) -> Option<Self> {
        let s = ppem / face.units_per_em() as f64;
        let bbox = |c: char| face.glyph_bounding_box(face.glyph_index(c)?);
        let (h, o, x, l, p) = (bbox('H')?, bbox('o')?, bbox('x')?, bbox('l'), bbox('p'));
        // (reference, overshoot): baseline, x-height, capitals, ascenders, descenders.
        let mut zones = vec![(h.y_min, o.y_min), (x.y_max, o.y_max)];
        zones.extend(bbox('O').map(|c| (h.y_max, c.y_max)));
        zones.extend(l.map(|l| (l.y_max, l.y_max)));
        zones.extend(p.map(|p| (p.y_min, p.y_min)));
        let scaled = (o.y_max as f64 * s * 64.0).round() as i64;
        let fitted = (scaled + 40) & !63;
        let sv = if scaled > 0 && fitted > 0 {
            s * fitted as f64 / scaled as f64
        } else {
            s
        };
        let mut knots = vec![];
        for (r, shoot) in zones {
            let fit = (r as f64 * sv).round();
            let d = (shoot - r) as f64 * sv * 64.0;
            let df: f64 = match d.abs() {
                a if a < 32.0 => 0.0,
                a if a < 48.0 => 32.0,
                _ => 64.0,
            };
            knots.push((r as f64, fit));
            knots.push((shoot as f64, fit + df.copysign(d) / 64.0));
        }
        knots.sort_by(|a, b| a.0.total_cmp(&b.0));
        knots.dedup_by(|a, b| a.0 == b.0);
        Some(Self { knots, scale: sv })
    }

    /// Pixels above the baseline for font-unit height `v`.
    fn y(&self, v: f64) -> f64 {
        let k = &self.knots;
        let i = k.partition_point(|p| p.0 <= v);
        match (i.checked_sub(1).map(|j| k[j]), k.get(i)) {
            (Some(a), Some(b)) => a.1 + (v - a.0) * (b.1 - a.1) / (b.0 - a.0),
            (Some(a), None) => a.1 + (v - a.0) * self.scale,
            (None, Some(b)) => b.1 + (v - b.0) * self.scale,
            (None, None) => v * self.scale,
        }
    }
}

/// One outline point, glyph-local in user space (y down): what it ends or controls.
#[derive(Clone, Copy)]
enum Pt {
    Move(f64, f64),
    Line(f64, f64),
    Ctrl(f64, f64),
    Quad(f64, f64),
    Cubic(f64, f64),
}

impl Pt {
    fn xy(self) -> (f64, f64) {
        match self {
            Pt::Move(x, y) | Pt::Line(x, y) | Pt::Ctrl(x, y) | Pt::Quad(x, y) | Pt::Cubic(x, y) => {
                (x, y)
            }
        }
    }
    fn with(self, (x, y): (f64, f64)) -> Pt {
        match self {
            Pt::Move(..) => Pt::Move(x, y),
            Pt::Line(..) => Pt::Line(x, y),
            Pt::Ctrl(..) => Pt::Ctrl(x, y),
            Pt::Quad(..) => Pt::Quad(x, y),
            Pt::Cubic(..) => Pt::Cubic(x, y),
        }
    }
}

struct Outline<'h> {
    /// Contours of glyph-local points: font units (y up) → user space (y down): y' = −s·y (or
    /// the hinted height divided by `hint.1`, the device y scale), x' = s·x + k·y', k being
    /// Skia's skewX.
    contours: Vec<Vec<Pt>>,
    s: f64,
    k: f64,
    hint: Option<(&'h Hinter, f64)>,
}

impl Outline<'_> {
    fn p(&self, x: f32, y: f32) -> (f64, f64) {
        let (x, y) = (x as f64, y as f64);
        let ly = self.hint.map_or(-self.s * y, |(h, dy)| -h.y(y) / dy);
        (self.s * x + self.k * ly, ly)
    }
    fn push(&mut self, pt: Pt) {
        if let Some(c) = self.contours.last_mut() {
            c.push(pt);
        }
    }
}

impl OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.p(x, y);
        self.contours.push(vec![Pt::Move(x, y)]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.p(x, y);
        self.push(Pt::Line(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, e) = (self.p(x1, y1), self.p(x, y));
        self.push(Pt::Ctrl(c.0, c.1));
        self.push(Pt::Quad(e.0, e.1));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (a, b, e) = (self.p(x1, y1), self.p(x2, y2), self.p(x, y));
        self.push(Pt::Ctrl(a.0, a.1));
        self.push(Pt::Ctrl(b.0, b.1));
        self.push(Pt::Cubic(e.0, e.1));
    }
    fn close(&mut self) {}
}

/// Grows the outline by `width / 2` on every side: the boundary of Skia's stroke fake bold
/// (outline filled together with its miter-joined stroke of `width`). The offset is computed
/// like FreeType's `FT_Outline_EmboldenXY`: each point moves outward along its corner's bisector
/// by `width / 2`, shortened on collapsing segments; FreeType's final (`width / 2`, `width / 2`)
/// translation is not applied, because a stroke grows on all sides.
///
/// Evidence (mean premultiplied diff inside the ink bbox; bold-16 / bold-italic-16 / bold-34):
/// this model 7.56 / 3.88 / 5.18. FreeType's strength (⌊ppem·64/24⌋ in 26.6) through the same
/// offset: 8.49 / 4.49 / 10.43 (too heavy at 34px). FreeType including its translation:
/// 32.14 / 26.50 at 16px. Stroking with tiny-skia and unioning coverages: 14.71 / 11.39 / 6.84,
/// because exact-area coverages cannot be unioned per pixel when fill and stroke both partly
/// cover it. Known: these are Skia's stroke fake-bold rule and FreeType's embolden. Inferred,
/// not verified in Chromium's sources here: Chromium rasterizes these webfont glyphs through
/// Skia's own path backend (Fontations), which uses the stroke rule and keeps the A8 mask gamma.
/// Works on y-up coordinates.
fn embolden(contours: &mut [Vec<Pt>], width: f64) {
    let half = width / 2.0;
    // FT_Outline_Get_Orientation: the shoelace sign over all contours (y up).
    let up = |p: Pt| {
        let (x, y) = p.xy();
        (x, -y)
    };
    let mut area = 0.0;
    for c in contours.iter() {
        for (i, p) in c.iter().enumerate() {
            let (q, r) = (up(c[(i + c.len() - 1) % c.len()]), up(*p));
            area += (r.1 - q.1) * (r.0 + q.0);
        }
    }
    if area == 0.0 || half == 0.0 {
        return;
    }
    let truetype = area < 0.0;
    let unit = |v: (f64, f64)| {
        let l = v.0.hypot(v.1);
        (if l == 0.0 { v } else { (v.0 / l, v.1 / l) }, l)
    };
    for c in contours.iter_mut() {
        let n = c.len();
        if n < 2 {
            continue;
        }
        let orig: Vec<(f64, f64)> = c.iter().map(|p| up(*p)).collect();
        let mut moved = orig.clone();
        let next = |j: usize| if j + 1 < n { j + 1 } else { 0 };
        let (mut inv, mut l_in) = ((0.0, 0.0), 0.0);
        let (mut anchor, mut l_anchor) = ((0.0, 0.0), 0.0);
        // j cycles through the points; i advances only when points move; k is the first moved.
        let (mut i, mut j, mut k) = (n - 1, 0, None::<usize>);
        while j != i && Some(i) != k {
            let (out, l_out) = if Some(j) != k {
                let (o, l) = unit((orig[j].0 - orig[i].0, orig[j].1 - orig[i].1));
                if l == 0.0 {
                    j = next(j);
                    continue;
                }
                (o, l)
            } else {
                (anchor, l_anchor)
            };
            if l_in != 0.0 {
                if k.is_none() {
                    (k, anchor, l_anchor) = (Some(i), inv, l_in);
                }
                let d = inv.0 * out.0 + inv.1 * out.1;
                // Shift only if the turn is less than ~160 degrees.
                let shift = if d > -0xF000 as f64 / 65536.0 {
                    let d = d + 1.0;
                    let (mut sx, mut sy) = (inv.1 + out.1, inv.0 + out.0);
                    if truetype {
                        sx = -sx;
                    } else {
                        sy = -sy;
                    }
                    let mut q = out.0 * inv.1 - out.1 * inv.0;
                    if truetype {
                        q = -q;
                    }
                    // Restrict the shift on collapsing segments.
                    let l = l_in.min(l_out);
                    let cap = |v: f64| {
                        if half * q <= l * d {
                            v * half / d
                        } else {
                            v * l / q
                        }
                    };
                    (cap(sx), cap(sy))
                } else {
                    (0.0, 0.0)
                };
                while i != j {
                    // FreeType also adds (half, half) here; a stroke grows on all sides.
                    moved[i] = (orig[i].0 + shift.0, orig[i].1 + shift.1);
                    i = next(i);
                }
            } else {
                i = j;
            }
            (inv, l_in) = (out, l_out);
            j = next(j);
        }
        for (p, m) in c.iter_mut().zip(moved) {
            *p = p.with((m.0, -m.1));
        }
    }
}

/// Lookup indices of the GPOS `kern` features, ascending (application order).
/// ponytail: only pair-adjustment lookups are applied; contextual kerning (lookup type 8) and
/// the second glyph's value record are skipped.
fn gpos_kern_lookups(face: &Face) -> Vec<u16> {
    let Some(gpos) = face.tables().gpos else {
        return vec![];
    };
    let kern = Tag::from_bytes(b"kern");
    let mut lookups: Vec<u16> = gpos
        .features
        .into_iter()
        .filter(|f| f.tag == kern)
        .flat_map(|f| f.lookup_indices)
        .collect();
    lookups.sort_unstable();
    lookups.dedup();
    lookups
}

/// x-advance adjustment of the first glyph, when this pair subtable applies to (l, r).
fn pair_advance(p: &PairAdjustment, l: GlyphId, r: GlyphId) -> Option<i32> {
    let v = match p {
        PairAdjustment::Format1 { coverage, sets } => sets.get(coverage.get(l)?)?.get(r)?.0,
        PairAdjustment::Format2 {
            coverage,
            classes,
            matrix,
        } => {
            coverage.get(l)?;
            matrix.get((classes.0.get(l), classes.1.get(r)))?.0
        }
    };
    Some(v.x_advance as i32)
}

/// `ctm`: when drawing, the CTM. Under an axis-aligned CTM, Skia's glyph positioning snaps the
/// device baseline to a whole pixel and each glyph origin to a quarter pixel (text-noto-*).
pub(super) fn layout(
    text: &str,
    font: &Font,
    align: TextAlign,
    baseline: TextBaseline,
    x: f64,
    y: f64,
    ctm: Option<&[f64; 6]>,
) -> Layout {
    let bytes = font_bytes(&font.family);
    let face = Face::parse(&bytes, 0).expect("registered fonts parse");
    let s = font.size_px / face.units_per_em() as f64;
    let gpos_kern = gpos_kern_lookups(&face);
    let kern = |l: GlyphId, r: GlyphId| -> i32 {
        if let Some(gpos) = face.tables().gpos {
            return gpos_kern
                .iter()
                .filter_map(|&i| gpos.lookups.get(i))
                .filter_map(|lookup| {
                    // The first subtable that applies to the pair wins within a lookup.
                    lookup
                        .subtables
                        .into_iter::<PositioningSubtable>()
                        .find_map(|t| match t {
                            PositioningSubtable::Pair(p) => pair_advance(&p, l, r),
                            _ => None,
                        })
                })
                .sum();
        }
        face.tables().kern.map_or(0, |k| {
            k.subtables
                .into_iter()
                .filter(|t| t.horizontal && !t.variable && !t.has_cross_stream)
                .find_map(|t| t.glyphs_kerning(l, r))
                .unwrap_or(0) as i32
        })
    };
    // Pen positions in font units.
    let glyphs: Vec<GlyphId> = text
        .chars()
        .map(|c| face.glyph_index(c).unwrap_or(GlyphId(0)))
        .collect();
    let mut pens = Vec::with_capacity(glyphs.len());
    let mut pen = 0i32;
    for (i, &g) in glyphs.iter().enumerate() {
        if i > 0 {
            pen += kern(glyphs[i - 1], g);
        }
        pens.push(pen);
        pen += face.glyph_hor_advance(g).unwrap_or(0) as i32;
    }
    let width = pen as f64 * s;
    let left = match align {
        TextAlign::Start | TextAlign::Left => x,
        TextAlign::Center => x - width / 2.0,
        TextAlign::End | TextAlign::Right => x - width,
    };
    let mut base = y + baseline_shift(&face, font.size_px, baseline);
    let axis_aligned = ctm.filter(|m| m[1] == 0.0 && m[2] == 0.0 && m[0] != 0.0 && m[3] != 0.0);
    if let Some(m) = axis_aligned {
        base = ((m[3] * base + m[5] + 0.5).floor() - m[5]) / m[3];
    }
    let k = if font.italic { FAKE_ITALIC_SKEW } else { 0.0 };
    // Hinted at the device size; measureText (no CTM) measures hinted glyphs at the font size.
    // ponytail: rotated, skewed or flipped CTMs render unhinted.
    let y_scale = match ctm {
        None => Some(1.0),
        Some(_) => axis_aligned.map(|m| m[3]).filter(|d| *d > 0.0),
    };
    // Fake italic (a skew transform) renders unhinted: text-noto-italic-16 diff 3.64 unhinted,
    // 12.53 hinted.
    let hinter = y_scale
        .filter(|_| !font.italic)
        .and_then(|d| Some((Hinter::new(&face, font.size_px * d)?, d)));
    // Skia emboldens the loaded (hinted) outline in device pixels; strength in user units.
    let device_scale = y_scale.unwrap_or_else(|| {
        let m = ctm.unwrap();
        (m[0] * m[3] - m[1] * m[2]).abs().sqrt()
    });
    // Synthetic bold: Skia's stroke width at the device size, in user units.
    let bold = (font.bold && device_scale > 0.0)
        .then(|| fake_bold_width(font.size_px * device_scale) / device_scale);
    let mut glyph_paths = Vec::with_capacity(glyphs.len());
    // Ink box relative to (left, base): left, top, right, bottom (y down).
    let mut ink = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (&g, &pen) in glyphs.iter().zip(&pens) {
        let pen = pen as f64 * s;
        let mut ox = left + pen;
        if let Some(m) = axis_aligned {
            ox = (((m[0] * ox + m[4]) * 4.0 + 0.5).floor() / 4.0 - m[4]) / m[0];
        }
        let mut b = Outline {
            contours: vec![],
            s,
            k,
            hint: hinter.as_ref().map(|(h, d)| (h, *d)),
        };
        face.outline_glyph(g, &mut b);
        if let Some(width) = bold {
            embolden(&mut b.contours, width);
        }
        let mut path = Path2D::new();
        let mut cbox = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for c in &b.contours {
            let mut pending: Vec<(f64, f64)> = vec![];
            for pt in c {
                let (lx, ly) = pt.xy();
                cbox = [
                    cbox[0].min(lx),
                    cbox[1].min(ly),
                    cbox[2].max(lx),
                    cbox[3].max(ly),
                ];
                let (x, y) = (ox + lx, base + ly);
                match pt {
                    Pt::Move(..) => path.move_to(x, y),
                    Pt::Line(..) => path.line_to(x, y),
                    Pt::Ctrl(..) => pending.push((x, y)),
                    Pt::Quad(..) => path.quadratic_curve_to(pending[0].0, pending[0].1, x, y),
                    Pt::Cubic(..) => {
                        let (a, c) = (pending[0], pending[1]);
                        path.bezier_curve_to(a.0, a.1, c.0, c.1, x, y);
                    }
                }
                if !matches!(pt, Pt::Ctrl(..)) {
                    pending.clear();
                }
            }
            path.close_path();
        }
        if cbox[0].is_finite() {
            // Chromium measures FreeType glyph boxes, rounded out to whole pixels per glyph;
            // vertically those are hinted, so ascent/descent can be 1px off (canvas-measure-text).
            ink = [
                ink[0].min(pen + cbox[0].floor()),
                ink[1].min(cbox[1].floor()),
                ink[2].max(pen + cbox[2].ceil()),
                ink[3].max(cbox[3].ceil()),
            ];
        }
        glyph_paths.push(path);
    }
    if !ink[0].is_finite() {
        ink = [0.0; 4];
    }
    let shift = base - y;
    Layout {
        glyphs: glyph_paths,
        metrics: TextMetrics {
            width,
            actual_bounding_box_left: x - (left + ink[0]),
            actual_bounding_box_right: left + ink[2] - x,
            actual_bounding_box_ascent: -ink[1] - shift,
            actual_bounding_box_descent: ink[3] + shift,
        },
    }
}

/// Exact-area coverage of device-space `path` over `bbox`, which may cut the path (nonzero,
/// |winding| clamped to 1), as
/// FreeType's gray rasterizer renders Chromium's glyph masks: 8-bit, `min(255, ⌊256·area⌋)`.
/// tiny-skia's 4× supersampling quantizes edge coverage to 1/16 steps instead.
pub(super) fn coverage(path: &tiny_skia::Path, bbox: tiny_skia::IntRect) -> Vec<u8> {
    let (w, h) = (bbox.width() as usize, bbox.height() as usize);
    let stride = w + 2;
    let mut acc = vec![0f32; stride * h];
    let (bx, by) = (bbox.x() as f32, bbox.y() as f32);
    let mut add = |a: (f32, f32), b: (f32, f32)| {
        let (a, b) = ((a.0 - bx, a.1 - by), (b.0 - bx, b.1 - by));
        // Split at the bbox's left and right edges: the parts outside become vertical runs on
        // the edge, which keeps the winding the visible pixels see exact.
        let mut cuts = [0.0, 1.0, 1.0, 1.0];
        let mut n = 2;
        for edge in [0.0, w as f32] {
            if (a.0 - edge) * (b.0 - edge) < 0.0 {
                cuts[n] = (edge - a.0) / (b.0 - a.0);
                n += 1;
            }
        }
        cuts[..n].sort_by(f32::total_cmp);
        let at = |t: f32| {
            (
                (a.0 + (b.0 - a.0) * t).clamp(0.0, w as f32),
                a.1 + (b.1 - a.1) * t,
            )
        };
        for t in cuts[..n].windows(2) {
            accumulate(&mut acc, stride, w, h, at(t[0]), at(t[1]));
        }
    };
    // Bézier point for control points `p` at t (Bernstein weights `wt`).
    let bez = |p: &[(f32, f32)], wt: &[f32]| {
        p.iter()
            .zip(wt)
            .fold((0.0, 0.0), |acc, (q, w)| (acc.0 + q.0 * w, acc.1 + q.1 * w))
    };
    // Second differences bound a curve's distance from its chord.
    let dd = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| {
        (a.0 - 2.0 * b.0 + c.0).hypot(a.1 - 2.0 * b.1 + c.1)
    };
    // Curves are flattened to within ~0.05px.
    let (mut start, mut cur) = ((0.0, 0.0), (0.0, 0.0));
    for seg in path.segments() {
        use tiny_skia::PathSegment as S;
        let pt = |p: tiny_skia::Point| (p.x, p.y);
        match seg {
            S::MoveTo(p) => {
                add(cur, start);
                (start, cur) = (pt(p), pt(p));
            }
            S::LineTo(p) => {
                add(cur, pt(p));
                cur = pt(p);
            }
            S::QuadTo(c, p) => {
                let ctrl = [cur, pt(c), pt(p)];
                let n = (dd(ctrl[0], ctrl[1], ctrl[2]) * 5.0).sqrt().ceil().max(1.0) as usize;
                let mut prev = cur;
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    let q = bez(&ctrl, &[u * u, 2.0 * u * t, t * t]);
                    add(prev, q);
                    prev = q;
                }
                cur = pt(p);
            }
            S::CubicTo(c1, c2, p) => {
                let ctrl = [cur, pt(c1), pt(c2), pt(p)];
                let d = dd(ctrl[0], ctrl[1], ctrl[2]).max(dd(ctrl[1], ctrl[2], ctrl[3]));
                let n = (d * 15.0).sqrt().ceil().max(1.0) as usize;
                let mut prev = cur;
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    let q = bez(
                        &ctrl,
                        &[u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t],
                    );
                    add(prev, q);
                    prev = q;
                }
                cur = pt(p);
            }
            S::Close => {
                add(cur, start);
                cur = start;
            }
        }
    }
    add(cur, start);
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        let mut sum = 0f32;
        for x in 0..w {
            sum += acc[y * stride + x];
            out[y * w + x] = (sum.abs().min(1.0) * 256.0).min(255.0) as u8;
        }
    }
    out
}

/// One line's signed area contributions (font-rs accumulation). Coordinates are bbox-relative.
fn accumulate(acc: &mut [f32], stride: usize, w: usize, h: usize, p0: (f32, f32), p1: (f32, f32)) {
    if p0.1 == p1.1 {
        return;
    }
    let (dir, p0, p1) = if p0.1 < p1.1 {
        (1.0, p0, p1)
    } else {
        (-1.0, p1, p0)
    };
    let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
    let (top, bottom) = (p0.1.max(0.0), p1.1.min(h as f32));
    let mut x = p0.0 + (top - p0.1) * dxdy;
    let mut row = top.floor() as usize;
    while (row as f32) < bottom {
        let line = row * stride;
        let dy = ((row + 1) as f32).min(bottom) - (row as f32).max(top);
        let xnext = x + dxdy * dy;
        let d = dy * dir;
        let (x0, x1) = if x < xnext { (x, xnext) } else { (xnext, x) };
        let (x0, x1) = (x0.clamp(0.0, w as f32), x1.clamp(0.0, w as f32));
        let x0floor = x0.floor();
        let x0i = x0floor as usize;
        let x1ceil = x1.ceil();
        let x1i = x1ceil as usize;
        if x1i <= x0i + 1 {
            let xmf = 0.5 * (x0 + x1) - x0floor;
            acc[line + x0i] += d - d * xmf;
            acc[line + x0i + 1] += d * xmf;
        } else {
            let s = (x1 - x0).recip();
            let x0f = x0 - x0floor;
            let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
            let x1f = x1 - x1ceil + 1.0;
            let am = 0.5 * s * x1f * x1f;
            acc[line + x0i] += d * a0;
            if x1i == x0i + 2 {
                acc[line + x0i + 1] += d * (1.0 - a0 - am);
            } else {
                let a1 = s * (1.5 - x0f);
                acc[line + x0i + 1] += d * (a1 - a0);
                for v in &mut acc[line + x0i + 2..line + x1i - 1] {
                    *v += d * s;
                }
                let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                acc[line + x1i - 1] += d * (1.0 - a2 - am);
            }
            acc[line + x1i] += d * am;
        }
        x = xnext;
        row += 1;
    }
}

/// Skia's A8 glyph-mask pre-blend (`SkTMaskGamma_build_correcting_lut`) for a paint of
/// luminance `lum`, quantized to Skia's 3 luminance bits, with Chromium's Linux text contrast
/// and gamma.
pub(super) fn mask_gamma(lum: u8) -> [u8; 256] {
    let i = lum >> 5;
    let lum = (i << 5) | (i << 2) | (i >> 1);
    let (contrast, gamma) = (TEXT_CONTRAST, TEXT_GAMMA);
    let src = lum as f32 / 255.0;
    let lin_src = src.powf(gamma);
    let dst = 1.0 - src;
    let lin_dst = dst.powf(gamma);
    // Contrast tapers off as the source luminance becomes white.
    let adjusted = contrast * lin_dst;
    std::array::from_fn(|i| {
        let raw = i as f32 / 255.0;
        let srca = raw + (1.0 - raw) * adjusted * raw;
        let v = if (src - dst).abs() < 1.0 / 256.0 {
            srca
        } else {
            let lin_out = lin_src * srca + (1.0 - srca) * lin_dst;
            (lin_out.powf(1.0 / gamma) - dst) / (src - dst)
        };
        (255.0 * v + 0.5).floor().clamp(0.0, 255.0) as u8
    })
}

/// Chromium's Linux text contrast and gamma (`SK_GAMMA_CONTRAST`, `SK_GAMMA_EXPONENT`).
const TEXT_CONTRAST: f32 = 0.2;
const TEXT_GAMMA: f32 = 1.2;
