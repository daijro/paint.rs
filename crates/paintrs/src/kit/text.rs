//! Chrome text in the UI font: paintdotjs's CSS line boxes (layout) with Paint.NET's text model
//! inside them (`ui-text-and-glyphs.md §2.4`; spec §0.5).
use std::sync::LazyLock;

use egui::{Color32, Context, FontFamily, FontId, Id, Painter, Pos2, Rect, Vec2};
use pdn_core::fonts::WinMetrics;

/// The UI face's layout metrics in em: hhea ascent and descent (Blink's line box; Selawik
/// 2027/2048 and 431/2048). Layout always measures the bundled metric-compatible face (spec §0.5),
/// which Chromium lays paintdotjs out with.
struct LayoutMetrics {
    ascent: f32,
    descent: f32,
}

static LAYOUT: LazyLock<LayoutMetrics> = LazyLock::new(|| {
    let face = ttf_parser::Face::parse(pdn_core::fonts::SELAWIK_REGULAR, 0).expect("Selawik");
    let upem = face.units_per_em() as f32;
    LayoutMetrics {
        ascent: face.ascender() as f32 / upem,
        descent: -(face.descender() as f32) / upem,
    }
});

#[derive(Clone)]
struct UiFace {
    family: String,
    ascent: f32,
    descent: f32,
    win: WinMetrics,
}
impl Default for UiFace {
    fn default() -> Self {
        Self {
            family: "Selawik".into(),
            ascent: LAYOUT.ascent,
            descent: LAYOUT.descent,
            win: WinMetrics {
                units_per_em: 2048,
                ascent: 2210,
                descent: 514,
            },
        }
    }
}
thread_local! {static UI_FACE:std::cell::RefCell<UiFace>=std::cell::RefCell::new(UiFace::default());}
/// Layout follows the selected bundled interface face. These metrics never consult host fonts.
pub fn set_ui_face(ctx: &Context, family: &str, bytes: &[u8]) {
    let Ok(face) = ttf_parser::Face::parse(bytes, 0) else {
        return;
    };
    let units = face.units_per_em() as f32;
    let metrics = UiFace {
        family: family.into(),
        ascent: face.ascender() as f32 / units,
        descent: -(face.descender() as f32) / units,
        win: pdn_core::fonts::win_metrics(bytes).unwrap_or(UiFace::default().win),
    };
    ctx.data_mut(|d| d.insert_temp(Id::unique("ui-measurement-face"), metrics.clone()));
    UI_FACE.with(|m| *m.borrow_mut() = metrics);
}
pub fn activate_ui_face(ctx: &Context) {
    if let Some(metrics) = ctx.data(|d| d.get_temp::<UiFace>(Id::unique("ui-measurement-face"))) {
        UI_FACE.with(|m| *m.borrow_mut() = metrics);
    }
}
pub fn ui_metrics() -> WinMetrics {
    UI_FACE.with(|m| m.borrow().win)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontSpec {
    /// CSS font-size in px, already multiplied by any CSS `zoom`.
    pub size: f32,
    /// CSS line-height in px; `None` = `normal`.
    pub line_height: Option<f32>,
    /// CSS font-weight: 700 draws in the UI bold face (`FontFamily::Name("ui-bold")`), 600 in
    /// the semibold one (`"ui-semibold"`), as Chromium picks Selawik's faces.
    pub weight: u16,
    /// CSS letter-spacing, used by Settings navigation group captions.
    pub letter_spacing: f32,
    /// A bundled text-tool face (`kit::fonts`) instead of the UI font, with its ascent and
    /// descent in em.
    pub face: Option<(&'static str, f32, f32)>,
}

impl FontSpec {
    pub const fn new(size: f32) -> Self {
        Self {
            size,
            line_height: None,
            weight: 400,
            letter_spacing: 0.,
            face: None,
        }
    }
    /// CSS explicitly asks for Segoe UI in size inputs, independent of the UI setting.
    pub fn segoe_ui(size: f32) -> Self {
        Self::new(size).face("Segoe UI", LAYOUT.ascent, LAYOUT.descent)
    }
    /// Text in a `kit::fonts` family with the given em ascent and descent.
    pub const fn face(mut self, family: &'static str, ascent_em: f32, descent_em: f32) -> Self {
        self.face = Some((family, ascent_em, descent_em));
        self
    }
    pub const fn line_height(mut self, lh: f32) -> Self {
        self.line_height = Some(lh);
        self
    }
    pub const fn spacing(mut self, spacing: f32) -> Self {
        self.letter_spacing = spacing;
        self
    }
    /// `font-weight: 700`.
    pub const fn bold(self) -> Self {
        self.weight(700)
    }
    pub const fn weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }
    /// Blink's line-box ascent at device scale `ppp` (zoom for DSF: the font is `size · ppp`
    /// device px), rounded to whole device pixels separately from the descent
    /// (SimpleFontData::PlatformInit); points.
    pub fn layout_ascent(&self, ppp: f32) -> f32 {
        (self.size
            * ppp
            * self
                .face
                .map_or_else(|| UI_FACE.with(|m| m.borrow().ascent), |f| f.1))
        .round()
            / ppp
    }
    pub fn layout_descent(&self, ppp: f32) -> f32 {
        (self.size
            * ppp
            * self
                .face
                .map_or_else(|| UI_FACE.with(|m| m.borrow().descent), |f| f.2))
        .round()
            / ppp
    }
    pub fn resolved_line_height(&self, ppp: f32) -> f32 {
        self.line_height
            .unwrap_or(self.layout_ascent(ppp) + self.layout_descent(ppp))
    }
    /// Baseline offset from the top of the CSS line box at device scale `ppp`, a whole number of
    /// device pixels. UI-font text follows Paint.NET: `snap((H − L)/2 + A)` in device px with the
    /// UI face's win metrics (`ui-text-and-glyphs.md §2.4` step 8), snapped by floor until capture
    /// B-T3 settles Direct2D's rule (Q3; controller ruling on task 17b: nearest put 61 chrome text
    /// rects 1 px below the references, floor matches them). `face` text (font previews) keeps
    /// Blink's half-leading, floored like LayoutNG, in device px.
    pub fn baseline(&self, ppp: f32) -> f32 {
        let h = self.resolved_line_height(ppp) * ppp;
        if self.face.is_some_and(|f| f.0 != "Segoe UI") {
            let (a, d) = (
                self.layout_ascent(ppp) * ppp,
                self.layout_descent(ppp) * ppp,
            );
            return (((h - (a + d)) / 2.0).floor() + a) / ppp;
        }
        let m = if self.face.is_some_and(|f| f.0 == "Segoe UI") {
            UiFace::default().win
        } else {
            ui_metrics()
        };
        let em = self.size * ppp / m.units_per_em as f32;
        let (a, l) = (m.ascent as f32 * em, (m.ascent + m.descent) as f32 * em);
        // calibrate: B-T3 (Appendix B) — floor per the task 17b controller ruling.
        ((h - l) / 2.0 + a).floor() / ppp
    }
    pub(crate) fn font_id(&self) -> FontId {
        let family = match self.face {
            Some((family, ..)) => FontFamily::Name(family.into()),
            None if self.weight >= 700 => FontFamily::Name("ui-bold".into()),
            None if self.weight >= 600 => FontFamily::Name("ui-semibold".into()),
            None if self.weight >= 500 => FontFamily::Name("ui-medium".into()),
            None => FontFamily::Proportional,
        };
        FontId::new(self.size, family)
    }
}

pub fn measure(painter: &Painter, text: &str, spec: FontSpec) -> f32 {
    let mut job =
        egui::text::LayoutJob::simple_singleline(text.to_owned(), spec.font_id(), Color32::WHITE);
    job.sections[0].format.extra_letter_spacing = spec.letter_spacing;
    painter.layout_job(job).size().x
}

/// Draws `text` in a CSS line box whose top-left is `top_left`. Returns the line-box rect.
pub fn draw(painter: &Painter, top_left: Pos2, text: &str, spec: FontSpec, color: Color32) -> Rect {
    draw_decorated(painter, top_left, text, spec, color, Decoration::default())
}

/// CSS logical start or explicit physical text alignment.
#[derive(Clone, Copy)]
pub enum Alignment {
    Start,
    Left,
    Right,
    Center,
}

fn aligned_x(painter: &Painter, rect: Rect, width: f32, alignment: Alignment) -> f32 {
    match alignment {
        Alignment::Start if crate::language::is_rtl(painter.ctx()) => rect.max.x - width,
        Alignment::Right => rect.max.x - width,
        Alignment::Center => rect.center().x - width / 2.,
        Alignment::Start | Alignment::Left => rect.min.x,
    }
}

/// Draw a line within its CSS box. Only inherited start alignment follows `dir`.
pub fn draw_aligned(
    painter: &Painter,
    rect: Rect,
    text: &str,
    spec: FontSpec,
    color: Color32,
    alignment: Alignment,
) -> Rect {
    let x = aligned_x(painter, rect, measure(painter, text, spec), alignment);
    draw(painter, Pos2::new(x, rect.min.y), text, spec, color)
}

/// One line of `text` in the box `rect` (its top-left the line box's), cut to the box's width
/// with `text-overflow: ellipsis`: Blink shapes the ellipsis on its own (no kerning against the
/// cut text) right after the text that fits beside it. Clipped to `rect`.
pub fn draw_ellipsized(painter: &Painter, rect: Rect, text: &str, spec: FontSpec, color: Color32) {
    draw_ellipsized_aligned(painter, rect, text, spec, color, Alignment::Start);
}

pub fn draw_ellipsized_aligned(
    painter: &Painter,
    rect: Rect,
    text: &str,
    spec: FontSpec,
    color: Color32,
    alignment: Alignment,
) {
    // Blink decides overflow in 1/64 px LayoutUnits using scale-independent UI advances.
    let ui_run = spec.face.is_none() && spec.weight < 600 && !uses_rtl_fallback(text);
    let width = |s: &str| {
        let w = if ui_run {
            css_width(s, spec.size)
        } else {
            measure(painter, s, spec)
        };
        (w * 64.0).round()
    };
    let room = (rect.width() * 64.0).round();
    let clip = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let mut shown = text.to_string();
    let ellipsized = width(text) > room;
    if ellipsized {
        let ellipsis = width("\u{2026}");
        while !shown.is_empty() && width(&shown) + ellipsis > room {
            shown.pop();
        }
    }
    let shown_width = measure(painter, &shown, spec);
    let ellipsis_width = if ellipsized {
        measure(painter, "\u{2026}", spec)
    } else {
        0.
    };
    let x = aligned_x(painter, rect, shown_width + ellipsis_width, alignment);
    let rtl = matches!(alignment, Alignment::Start) && crate::language::is_rtl(painter.ctx());
    if ellipsized {
        draw(
            &clip,
            Pos2::new(if rtl { x } else { x + shown_width }, rect.min.y),
            "\u{2026}",
            spec,
            color,
        );
    }
    draw(
        &clip,
        Pos2::new(x + if rtl { ellipsis_width } else { 0. }, rect.min.y),
        &shown,
        spec,
        color,
    );
}

/// Synthesized font styles and `text-decoration` lines.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Decoration {
    /// Skia's fake italic for the Regular-only font (egui skews the glyph quads by 1/4).
    pub italic: bool,
    /// `text-decoration: underline` with `text-underline-offset` (px below the baseline).
    pub underline: Option<f32>,
    /// `text-decoration: line-through`.
    pub strike: bool,
}

/// `draw` with `decoration`; the lines are recorded with the text.
pub fn draw_decorated(
    painter: &Painter,
    top_left: Pos2,
    text: &str,
    spec: FontSpec,
    color: Color32,
    decoration: Decoration,
) -> Rect {
    draw_as(
        painter,
        top_left,
        text,
        spec,
        color,
        decoration,
        RectKind::Text,
    )
}

/// `draw` for text Paint.NET draws itself (R21 (c)): its ink box is recorded as a pdn rect.
pub fn draw_pdn(painter: &Painter, top_left: Pos2, text: &str, spec: FontSpec, color: Color32) {
    draw_as(
        painter,
        top_left,
        text,
        spec,
        color,
        Decoration::default(),
        RectKind::Pdn,
    );
}

fn draw_as(
    painter: &Painter,
    top_left: Pos2,
    text: &str,
    spec: FontSpec,
    color: Color32,
    decoration: Decoration,
    kind: RectKind,
) -> Rect {
    let ppp = painter.pixels_per_point();
    // Chromium positions glyph runs at fractional x (Skia quarter-pixel subpixel glyphs); egui
    // rounds a galley's origin to whole pixels, so the fraction rides in as leading space.
    let x_device = top_left.x * ppp;
    let lead = (x_device - x_device.floor()) / ppp;
    let job = |color: Color32| {
        let mut job =
            egui::text::LayoutJob::simple_singleline(text.to_owned(), spec.font_id(), color);
        if let Some(section) = job.sections.first_mut() {
            section.leading_space = lead;
            section.format.extra_letter_spacing = spec.letter_spacing;
            section.format.italics = decoration.italic;
        }
        job
    };
    let galley = painter.layout_job(job(color));
    let egui_baseline = galley
        .rows
        .first()
        .map(|r| {
            r.pos.y
                + r.row
                    .glyphs
                    .first()
                    .map_or(spec.layout_ascent(ppp), |g| g.pos.y)
        })
        .unwrap_or(spec.layout_ascent(ppp));
    let snap = |v: f32| (v * ppp).round() / ppp;
    let baseline_y = snap(top_left.y + spec.baseline(ppp));
    let pos = Pos2::new(x_device.floor() / ppp, baseline_y - egui_baseline);
    let width = galley.size().x - lead;
    painter.galley(pos, galley, color);
    let line_box = Rect::from_min_size(top_left, Vec2::new(width, spec.resolved_line_height(ppp)));
    // Chrome text only: Blink's `text-decoration-thickness: auto`, font-size / 10, at least 1 px
    // (the references show it; chrome look is paintdotjs's, task 17b ruling). The Text tool's
    // decorations use the font's own metrics instead (`painting-tools-2.md §2.6` step 5).
    let auto_thickness = (spec.size / 10.0).max(1.0);
    let thickness = (auto_thickness * ppp).round().max(1.0) / ppp;
    // Decoration lines: whole-pixel rows, anti-aliased fractional ends (Skia fills the rect).
    let line = |y: f32| {
        let y = (y * ppp).round() / ppp;
        let (x0, x1) = (
            top_left.x * ppp,
            (top_left.x
                + if uses_rtl_fallback(text) {
                    width
                } else {
                    css_width(text, spec.size)
                })
                * ppp,
        );
        let mut x = x0.floor();
        while x < x1 {
            let cover = (x1.min(x + 1.0) - x0.max(x)).clamp(0.0, 1.0);
            let c = if cover >= 1.0 {
                color
            } else {
                color.gamma_multiply(cover)
            };
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(x / ppp, y), Vec2::new(1.0 / ppp, thickness)),
                0.0,
                c,
            );
            x += 1.0;
        }
    };
    if let Some(offset) = decoration.underline {
        line(baseline_y + offset);
    }
    if decoration.strike {
        // Blink's line-through is centered two thirds of the ascent below the text top.
        line(baseline_y - spec.layout_ascent(ppp) / 3.0 - thickness / 2.0);
    }
    let ink = Rect::from_min_max(
        Pos2::new(
            pos.x - 1.0,
            (baseline_y - spec.layout_ascent(ppp)).min(top_left.y) - 1.0,
        ),
        Pos2::new(
            // The fake-italic skew leans the tops right by a quarter of their height.
            pos.x
                + width
                + 2.0
                + if decoration.italic {
                    spec.layout_ascent(ppp) / 4.0
                } else {
                    0.0
                },
            (baseline_y + spec.layout_descent(ppp)).max(line_box.max.y) + 1.0,
        ),
    );
    record(
        painter.ctx(),
        ink.intersect(painter.clip_rect()),
        kind,
        Some(text),
    );
    line_box
}

/// Draws `text` rotated -90 degrees (reading upwards) from the baseline start `at`, as canvas
/// `translate(at); rotate(-PI / 2); fillText(text, 0, 0)` does. Chromium hints and subpixel-
/// positions glyphs along the rotated advance axis, so the galley origin is snapped to whole
/// pixels with the fractional start riding in as leading space (like `draw`), and the rotation
/// keeps every glyph quad pixel-aligned. Records nothing: the ruler labels sit in the rulers'
/// pdn rect.
pub fn draw_rotated_ccw(painter: &Painter, at: Pos2, text: &str, spec: FontSpec, color: Color32) {
    let ppp = painter.pixels_per_point();
    // Upwards = the galley's +x: start at the next whole pixel below and lead back up.
    let y_device = at.y * ppp;
    let lead = (y_device.ceil() - y_device) / ppp;
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), spec.font_id(), color);
    if let Some(section) = job.sections.first_mut() {
        section.leading_space = lead;
    }
    let galley = painter.layout_job(job);
    let egui_baseline = galley
        .rows
        .first()
        .map(|r| {
            r.pos.y
                + r.row
                    .glyphs
                    .first()
                    .map_or(spec.layout_ascent(ppp), |g| g.pos.y)
        })
        .unwrap_or(spec.layout_ascent(ppp));
    // The rotation maps the galley's (0, baseline) to (baseline, 0) from its origin.
    let snap = |v: f32| (v * ppp).round() / ppp;
    let pos = Pos2::new(snap(at.x - egui_baseline), y_device.ceil() / ppp);
    painter.add(
        egui::epaint::TextShape::new(pos, galley, color).with_angle(-std::f32::consts::FRAC_PI_2),
    );
}

/// What a recorded rect covers (layout tests read them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RectKind {
    Text,
    /// Pixels Paint.NET's logic computes; R21 (c).
    Pdn,
}

/// A recorded rect, with the text drawn in it (absent for non-text pdn rects).
type Recorded = (Rect, RectKind, Option<std::sync::Arc<str>>);

#[derive(Clone, Default)]
pub struct TextRects {
    pub enabled: bool,
    rects: Vec<Recorded>,
}

fn id() -> Id {
    Id::unique("paintrs.text_rects")
}

pub fn enable_text_recording(ctx: &Context) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<TextRects>(id()).enabled = true);
}

/// Clear at the start of every frame so only the final frame's rects are recorded.
pub fn begin_frame(ctx: &Context) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<TextRects>(id()).rects.clear());
}

/// Records `rect` (already cut to what is visible) and the text drawn in it; an empty one is
/// dropped.
fn record(ctx: &Context, rect: Rect, kind: RectKind, text: Option<&str>) {
    ctx.data_mut(|d| {
        let r = d.get_temp_mut_or_default::<TextRects>(id());
        if r.enabled && rect.is_positive() {
            r.rects.push((rect, kind, text.map(Into::into)));
        }
    });
}

pub fn record_text_rect(ctx: &Context, rect: Rect) {
    record(ctx, rect, RectKind::Text, None);
}

/// Records a screen rect (points) whose pixels Paint.NET's logic computes (canvas view, rulers,
/// colour wheel/sliders/swatches, thumbnails, previews): the diff skips them (R21 (c)).
pub fn record_pdn_rect(ctx: &Context, rect: Rect) {
    record(ctx, rect, RectKind::Pdn, None);
}

/// An opaque box (a popup) now covers `cover`: the text and canvas pixels recorded under it are
/// hidden, so their rects shrink to the parts outside it (the popup's own pixels compare exactly).
pub fn occlude(ctx: &Context, cover: Rect) {
    ctx.data_mut(|d| {
        let r = d.get_temp_mut_or_default::<TextRects>(id());
        if !r.enabled {
            return;
        }
        r.rects = r
            .rects
            .iter()
            .flat_map(|(rect, kind, text)| {
                subtract(*rect, cover)
                    .into_iter()
                    .map(move |p| (p, *kind, text.clone()))
            })
            .collect();
    });
}

/// Records `rect` (widened to whole device pixels) as Paint.NET's pixels over everything recorded
/// under it, text included (what lies outside keeps its own comparison): a translucent box over
/// Paint.NET's pixels, whose own pixels, text too, then blend them.
pub fn record_pdn_rect_over(ctx: &Context, rect: Rect) {
    let ppp = ctx.pixels_per_point();
    let rect = Rect::from_min_max(
        Pos2::new((rect.min.x * ppp).floor(), (rect.min.y * ppp).floor()) / ppp,
        Pos2::new((rect.max.x * ppp).ceil(), (rect.max.y * ppp).ceil()) / ppp,
    );
    occlude(ctx, rect);
    record_pdn_rect(ctx, rect);
}

/// `r` minus `hole`: up to four rects (above, below, left, right).
fn subtract(r: Rect, hole: Rect) -> Vec<Rect> {
    let h = r.intersect(hole);
    if !h.is_positive() {
        return vec![r];
    }
    let mut out = vec![];
    let mut push = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let p = Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1));
        if p.is_positive() {
            out.push(p);
        }
    };
    push(r.min.x, r.min.y, r.max.x, h.min.y);
    push(r.min.x, h.max.y, r.max.x, r.max.y);
    push(r.min.x, h.min.y, h.min.x, h.max.y);
    push(h.max.x, h.min.y, r.max.x, h.max.y);
    out
}

/// The text rects drawn since the last take, with each rect's kind and text (layout tests match
/// runs across scales).
pub fn take_text_runs(ctx: &Context) -> Vec<(Rect, RectKind, Option<String>)> {
    let rects =
        ctx.data_mut(|d| std::mem::take(&mut d.get_temp_mut_or_default::<TextRects>(id()).rects));
    rects
        .into_iter()
        .map(|(rect, kind, text)| (rect, kind, text.map(|t| t.to_string())))
        .collect()
}

/// Inline size of `text` in the active bundled UI family at `size` px: HarfBuzz advances with pair kerning (pdn-core's canvas shaper, unhinted),
/// rounded up to a LayoutUnit (1/64 px).
pub fn css_width(text: &str, size: f32) -> f32 {
    thread_local! {
        static CANVAS: std::cell::RefCell<pdn_core::canvas::Canvas> =
            std::cell::RefCell::new(pdn_core::canvas::Canvas::new(1, 1));
        /// Widths measured so far, by size and text, for one family and font generation: every
        /// frame measures the same labels, and a layout walks each glyph's outline.
        static WIDTHS: std::cell::RefCell<WidthCache> = Default::default();
    }
    let family = UI_FACE.with(|m| m.borrow().family.clone());
    let generation = pdn_core::canvas::text::font_generation();
    let known = WIDTHS.with_borrow_mut(|c| {
        if c.family != family || c.generation != generation {
            *c = WidthCache {
                family: family.clone(),
                generation,
                ..Default::default()
            };
        }
        c.widths.get(&size.to_bits())?.get(text).copied()
    });
    if let Some(w) = known {
        return w;
    }
    let w = CANVAS.with(|c| {
        let mut c = c.borrow_mut();
        c.set_font(pdn_core::canvas::Font {
            family,
            size_px: size as f64,
            bold: false,
            italic: false,
        });
        c.measure_text(text).width
    });
    // ponytail: Blink measures some exact 1/64 sums one LayoutUnit wider ("75%", "Width:",
    // "Height:"; HarfBuzz's float advances land a hair above) but not others (the toggles' "B"
    // centers as 13px); plain ceil keeps every scenario's pixels. Model HarfBuzz's advance
    // rounding if a layout lands on a pixel boundary.
    let w = ((w * 64.0).ceil() / 64.0) as f32;
    WIDTHS.with_borrow_mut(|c| {
        // ponytail: forgets everything past this many (texts that change, like the status bar's
        // coordinates, would grow it forever); an LRU if a frame ever measures more than this.
        if c.len > 4096 {
            c.widths.clear();
            c.len = 0;
        }
        c.len += 1;
        c.widths
            .entry(size.to_bits())
            .or_default()
            .insert(text.to_owned(), w);
    });
    w
}

#[derive(Default)]
struct WidthCache {
    family: String,
    generation: u64,
    len: usize,
    widths: std::collections::HashMap<u32, std::collections::HashMap<String, f32>>,
}

/// GDI's advance of `text` in the tool strip's 9-pt UI font at 96 DPI, in DIPs (R24): each glyph's
/// Selawik advance rounded to a whole pixel at 12 ppem, no kerning — Segoe UI's `hdmx` widths,
/// which Selawik's ASCII advances equal except 'C' (8) and 'm' (11) (`ui-text-and-glyphs.md
/// §8.2` "Text box T"). Only the strip's autosizing uses it; drawing keeps natural advances.
pub fn gdi_width(text: &str) -> f32 {
    static FACE: std::sync::LazyLock<ttf_parser::Face<'static>> = std::sync::LazyLock::new(|| {
        ttf_parser::Face::parse(pdn_core::fonts::SELAWIK_REGULAR, 0).expect("Selawik")
    });
    let em = FACE.units_per_em() as f32;
    text.chars()
        .map(|c| match c {
            'C' => 8.0,
            'm' => 11.0,
            _ => FACE
                .glyph_index(c)
                .and_then(|g| FACE.glyph_hor_advance(g))
                .map_or(0.0, |a| (a as f32 * 12.0 / em).round()),
        })
        .sum()
}

/// A styled piece of an inline paragraph (a DOM text node or inline element); `"\n"` in `text`
/// is a `<br>`.
#[derive(Clone, Copy, Debug)]
pub struct Run<'a> {
    pub text: &'a str,
    pub spec: FontSpec,
    pub color: Color32,
    /// `text-decoration: underline` (1 px under the baseline, as the dialog links show it).
    pub underline: bool,
}

/// Part of a run on a line: its x from the line's start, its width and text.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub run: usize,
    pub x: f32,
    pub width: f32,
    pub text: String,
}

fn uses_rtl_fallback(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '\u{0590}'..='\u{08ff}' | '\u{fb1d}'..='\u{fdff}' | '\u{fe70}'..='\u{feff}'))
}

/// Preserve Latin/CSS metrics; RTL fallback runs use the same shaping as their painted glyphs.
pub fn layout_width(painter: &Painter, text: &str, spec: FontSpec) -> f32 {
    if spec.face.is_none() && spec.weight < 600 && !uses_rtl_fallback(text) {
        css_width(text, spec.size)
    } else {
        measure(painter, text, spec)
    }
}

/// Blink's greedy line breaking of `runs` within `width` (`white-space: normal`): a break
/// opportunity after each space, a word (which may span runs) moves to the next line when it
/// would overflow, the spaces before a soft break hang (dropped), a word wider than the line
/// overflows it, `"\n"` forces a break. One `Vec` per line (an empty line keeps its height).
pub fn wrap_runs(painter: &Painter, runs: &[Run], width: f32) -> Vec<Vec<Placed>> {
    wrap_runs_aligned(painter, runs, width, Alignment::Start)
}

pub fn wrap_runs_aligned(
    painter: &Painter,
    runs: &[Run],
    width: f32,
    alignment: Alignment,
) -> Vec<Vec<Placed>> {
    let mut lines: Vec<Vec<Placed>> = vec![vec![]];
    let mut x = 0.0;
    let mut word: Vec<(usize, String)> = vec![];
    let place_word =
        |lines: &mut Vec<Vec<Placed>>, x: &mut f32, word: &mut Vec<(usize, String)>| {
            let pieces: Vec<(usize, String, f32)> = word
                .drain(..)
                .map(|(r, t)| {
                    let w = layout_width(painter, &t, runs[r].spec);
                    (r, t, w)
                })
                .collect();
            if pieces.is_empty() {
                return;
            }
            let w: f32 = pieces.iter().map(|p| p.2).sum();
            let line = lines.last_mut().expect("a line");
            if !line.is_empty() && *x + w > width {
                while line.last().is_some_and(|p| p.text.trim().is_empty()) {
                    line.pop();
                }
                lines.push(vec![]);
                *x = 0.0;
            }
            let line = lines.last_mut().expect("a line");
            for (run, text, w) in pieces {
                line.push(Placed {
                    run,
                    x: *x,
                    width: w,
                    text,
                });
                *x += w;
            }
        };
    for (r, run) in runs.iter().enumerate() {
        let mut rest = run.text;
        while !rest.is_empty() {
            let cut = rest.find([' ', '\n']).unwrap_or(rest.len());
            if cut > 0 {
                word.push((r, rest[..cut].to_string()));
            }
            rest = &rest[cut..];
            if let Some(c) = rest.chars().next() {
                place_word(&mut lines, &mut x, &mut word);
                if c == '\n' {
                    lines.push(vec![]);
                    x = 0.0;
                } else if !lines.last().expect("a line").is_empty() {
                    // A space at a line's start after a break collapses away.
                    let w = layout_width(painter, " ", run.spec);
                    lines.last_mut().expect("a line").push(Placed {
                        run: r,
                        x,
                        width: w,
                        text: " ".into(),
                    });
                    x += w;
                }
                rest = &rest[1..];
            }
        }
    }
    place_word(&mut lines, &mut x, &mut word);
    for line in &mut lines {
        let occupied = line.last().map_or(0., |p| p.x + p.width);
        let offset = aligned_x(
            painter,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 0.)),
            occupied,
            alignment,
        );
        for piece in line {
            piece.x += offset;
        }
    }
    lines
}

/// Draws `lines` (from `wrap_runs`) from `top_left`, one `line_height` apart. Each run's text box
/// (its `normal` line height) sits in the line box by Blink's half-leading, floored to whole
/// pixels as the paintdotjs DOM shows (18.6-px list lines: text 1 px down), and the glyphs take
/// Paint.NET's baseline in that box.
pub fn draw_lines(
    painter: &Painter,
    top_left: Pos2,
    runs: &[Run],
    lines: &[Vec<Placed>],
    line_height: f32,
) {
    for (i, line) in lines.iter().enumerate() {
        // A run's pieces on a line are contiguous: shape (and underline) them as one string.
        let mut groups: Vec<(usize, f32, String)> = vec![];
        for p in line {
            match groups.last_mut() {
                Some((run, _, text)) if *run == p.run => text.push_str(&p.text),
                _ => groups.push((p.run, p.x, p.text.clone())),
            }
        }
        for (r, x, text) in groups {
            let run = &runs[r];
            if text.trim().is_empty() && !run.underline {
                continue;
            }
            let spec = FontSpec {
                line_height: None,
                ..run.spec
            };
            let ppp = painter.pixels_per_point();
            let leading = ((line_height - spec.resolved_line_height(ppp)) / 2.0).floor();
            let at = top_left + Vec2::new(x, i as f32 * line_height + leading);
            let decoration = Decoration {
                underline: run.underline.then_some(1.0),
                ..Default::default()
            };
            draw_decorated(painter, at, &text, spec, run.color, decoration);
        }
    }
}

#[cfg(test)]
mod pdn_text_tests {
    use super::*;

    /// A kept width is the measured one, until the family's font changes.
    #[test]
    fn css_widths_are_kept_until_the_fonts_change() {
        // A family of this thread's own: other tests' fonts stay as they are.
        let family = "css-width-cache-test";
        UI_FACE.with(|m| m.borrow_mut().family = family.into());
        let measure = |size: f32| {
            let mut c = pdn_core::canvas::Canvas::new(1, 1);
            c.set_font(pdn_core::canvas::Font {
                family: family.into(),
                size_px: size as f64,
                bold: false,
                italic: false,
            });
            ((c.measure_text("Width:").width * 64.0).ceil() / 64.0) as f32
        };
        let noto = css_width("Width:", 12.0);
        assert_eq!(noto, measure(12.0));
        assert_eq!(css_width("Width:", 12.0), noto);
        assert_eq!(css_width("Width:", 24.0), measure(24.0));
        pdn_core::canvas::text::register_font(family, pdn_core::fonts::CARLITO_REGULAR.to_vec());
        let carlito = css_width("Width:", 12.0);
        pdn_core::canvas::text::unregister_font(family);
        assert_ne!(carlito, noto);
        assert_eq!(css_width("Width:", 12.0), noto);
    }

    /// `ui-text-and-glyphs.md §8.3`: A_gdi(12) of the paintdotjs-checked strings.
    #[test]
    fn gdi_widths_match_paint_net() {
        for (text, a) in [
            ("Layer", 28.0),
            ("Normal", 40.0),
            ("Any Size", 44.0),
            ("Do not switch tool", 97.0),
            ("Single Pixel", 60.0),
            ("Smooth", 42.0),
            ("Finish", 31.0),
            ("Rectangle", 52.0),
        ] {
            assert_eq!(gdi_width(text), a, "{text}");
        }
        // The hdmx exceptions.
        assert_eq!(gdi_width("C"), 8.0);
        assert_eq!(gdi_width("m"), 11.0);
    }

    /// ui-text-and-glyphs.md §5 "Task 17b" baseline vectors (Segoe UI = Selawik win metrics),
    /// floor-snapped per the task 17b ruling (B-T3 pending). L = size·2724/2048, A = size·2210/2048:
    /// 12 px → L 15.9609375, A 12.94921875; 15 px → L 19.951171875, A 16.1865234375;
    /// 14 px → L 18.62109375, A 15.107421875.
    #[test]
    fn baseline_follows_paint_net() {
        for (size, box_h, baseline) in [
            (12.0, 16.0, 12.0), // 0.01953125 + A = 12.96875
            (12.0, 20.0, 14.0), // 2.01953125 + A = 14.96875
            (12.0, 22.0, 15.0), // 3.01953125 + A = 15.96875
            (15.0, 20.0, 16.0), // 0.0244140625 + A = 16.2109375
            (15.0, 25.0, 18.0), // 2.5244140625 + A = 18.7109375
            (14.0, 17.0, 14.0), // -0.810546875 + A = 14.296875
        ] {
            assert_eq!(
                FontSpec::new(size).line_height(box_h).baseline(1.0),
                baseline,
                "{size}px in {box_h}"
            );
        }
    }

    #[test]
    fn ui_metrics_are_the_ui_faces_win_metrics() {
        let selawik = pdn_core::fonts::win_metrics(pdn_core::fonts::SELAWIK_REGULAR);
        assert_eq!(Some(ui_metrics()), selawik);
    }

    /// Blink's line box comes from Selawik's hhea (2027, -431 at upem 2048), and the Paint.NET
    /// baseline sits in it when `line-height` is `normal`.
    #[test]
    fn normal_line_height_uses_selawik_hhea() {
        assert_eq!(
            (LAYOUT.ascent * 2048.0, LAYOUT.descent * 2048.0),
            (2027.0, 431.0)
        );
        // 12 px: round(11.877) + round(2.525) = 12 + 3 = 15; (15 − 15.9609375)/2 + 12.94921875
        // = 12.469 → 12. 14 px: round(13.856) + round(2.946) = 14 + 3 = 17; 14.296875 → 14.
        for (size, line, baseline) in [(12.0, 15.0, 12.0), (14.0, 17.0, 14.0)] {
            let spec = FontSpec::new(size);
            assert_eq!(spec.resolved_line_height(1.0), line, "{size}px");
            assert_eq!(spec.baseline(1.0), baseline, "{size}px");
        }
    }

    /// Task 23: Blink rounds the line box at the device font size (the DOM's inline box at 12 px:
    /// 14.4 at DPR 1.25 = round(14.845) + round(3.157) = 18 device px, 14.5 at DPR 2 =
    /// round(23.75) + round(5.05) = 29), and the Paint.NET baseline floors in device px: 1.25 →
    /// (18 − 19.951171875)/2 + 16.1865234375 = 15.21 → 15 px; 2 → (29 − 31.921875)/2 +
    /// 25.8984375 = 24.44 → 24 px.
    #[test]
    fn line_metrics_round_at_the_device_scale() {
        let spec = FontSpec::new(12.0);
        assert_eq!(spec.resolved_line_height(1.25) * 1.25, 18.0);
        assert_eq!(spec.resolved_line_height(2.0), 14.5);
        assert_eq!(spec.baseline(1.25) * 1.25, 15.0);
        assert_eq!(spec.baseline(2.0) * 2.0, 24.0);
    }
}

#[cfg(test)]
mod shrink_tests {
    use super::*;
    #[test]
    fn shrink_to_fit_steps() {
        let never = |s: f32| (s * 100.0, s);
        assert!((shrink_to_fit(never, 16.0, 10.0, 100.0, 0.6) - 10.0).abs() < 1e-4);
        assert!((shrink_to_fit(never, 16.0, 10.0, 100.0, 0.25) - 4.4).abs() < 1e-4);
        assert!((shrink_to_fit(|s| (s, s), 16.0, 12.8, 100.0, 0.25) - 12.8).abs() < 1e-4);
    }
}

/// Paint.NET's shrink-to-fit text sizing, ui-text-and-glyphs §2.5. Keep the final size even on
/// overflow.
pub fn shrink_to_fit(
    measure: impl Fn(f32) -> (f32, f32),
    s0: f32,
    w: f32,
    h: f32,
    floor: f32,
) -> f32 {
    let n = ((100.0 * ((1.0 - floor) / (100.0 * 0.025))) as usize).max(5);
    let mut size = s0;
    for k in 0..n {
        let (mw, mh) = measure(size);
        if mw <= w && mh <= h {
            break;
        }
        let f = 1.0 - 0.025 * k as f32;
        if f < floor {
            break;
        }
        size = s0 * f;
    }
    size
}
