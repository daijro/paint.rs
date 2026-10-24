//! Port target for every Canvas 2D call in paintdotjs (spec §6.1). Only the surveyed subset exists.
pub mod blur;
mod image_data;
mod lowp;
pub mod path;
mod sampling;
pub mod text;

pub use image_data::{ImageData, premultiply, unpremultiply};
pub use path::Path2D;
pub use text::{Font, TextAlign, TextBaseline, TextMetrics};

use std::borrow::Cow;
use tiny_skia::{
    BlendMode, Color, FillRule as SkFillRule, FilterQuality, IntRect, Mask, Paint, PathBuilder,
    Pixmap, PixmapPaint, Rect, Shader, SpreadMode, Stroke, StrokeDash, Transform,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

impl FillRule {
    pub fn from_css(s: &str) -> Self {
        if s == "evenodd" {
            FillRule::EvenOdd
        } else {
            FillRule::NonZero
        }
    }
    fn skia(self) -> SkFillRule {
        match self {
            FillRule::NonZero => SkFillRule::Winding,
            FillRule::EvenOdd => SkFillRule::EvenOdd,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}
impl LineCap {
    pub fn from_css(s: &str) -> Self {
        match s {
            "round" => LineCap::Round,
            "square" => LineCap::Square,
            _ => LineCap::Butt,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}
impl LineJoin {
    pub fn from_css(s: &str) -> Self {
        match s {
            "round" => LineJoin::Round,
            "bevel" => LineJoin::Bevel,
            _ => LineJoin::Miter,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositeOp {
    SourceOver,
    Copy,
    SourceIn,
    SourceOut,
    SourceAtop,
    DestinationOver,
    DestinationIn,
    DestinationOut,
    DestinationAtop,
    Xor,
    Lighter,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
}

impl CompositeOp {
    pub fn from_css(s: &str) -> Option<Self> {
        use CompositeOp::*;
        Some(match s {
            "source-over" => SourceOver,
            "copy" => Copy,
            "source-in" => SourceIn,
            "source-out" => SourceOut,
            "source-atop" => SourceAtop,
            "destination-over" => DestinationOver,
            "destination-in" => DestinationIn,
            "destination-out" => DestinationOut,
            "destination-atop" => DestinationAtop,
            "xor" => Xor,
            "lighter" => Lighter,
            "multiply" => Multiply,
            "screen" => Screen,
            "overlay" => Overlay,
            "darken" => Darken,
            "lighten" => Lighten,
            "color-dodge" => ColorDodge,
            "color-burn" => ColorBurn,
            "hard-light" => HardLight,
            "soft-light" => SoftLight,
            "difference" => Difference,
            "exclusion" => Exclusion,
            _ => return None,
        })
    }
    fn skia(self) -> BlendMode {
        use CompositeOp::*;
        match self {
            SourceOver => BlendMode::SourceOver,
            Copy => BlendMode::Source,
            SourceIn => BlendMode::SourceIn,
            SourceOut => BlendMode::SourceOut,
            SourceAtop => BlendMode::SourceAtop,
            DestinationOver => BlendMode::DestinationOver,
            DestinationIn => BlendMode::DestinationIn,
            DestinationOut => BlendMode::DestinationOut,
            DestinationAtop => BlendMode::DestinationAtop,
            Xor => BlendMode::Xor,
            Lighter => BlendMode::Plus,
            Multiply => BlendMode::Multiply,
            Screen => BlendMode::Screen,
            Overlay => BlendMode::Overlay,
            Darken => BlendMode::Darken,
            Lighten => BlendMode::Lighten,
            ColorDodge => BlendMode::ColorDodge,
            ColorBurn => BlendMode::ColorBurn,
            HardLight => BlendMode::HardLight,
            SoftLight => BlendMode::SoftLight,
            Difference => BlendMode::Difference,
            Exclusion => BlendMode::Exclusion,
        }
    }
    /// Canvas "uncontained" operators affect pixels outside the source shape.
    fn uncontained(self) -> bool {
        use CompositeOp::*;
        matches!(
            self,
            Copy | SourceIn | SourceOut | DestinationIn | DestinationAtop
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SmoothingQuality {
    #[default]
    Low,
    Medium,
    High,
}

impl SmoothingQuality {
    pub fn from_css(s: &str) -> Self {
        match s {
            "medium" => SmoothingQuality::Medium,
            "high" => SmoothingQuality::High,
            _ => SmoothingQuality::Low,
        }
    }
}

/// Gradient color stops: unpremultiplied RGBA, sorted by offset (a stop goes after existing
/// stops with an equal offset, like `addColorStop`).
type Stops = Vec<(f64, [u8; 4])>;

fn add_stop(stops: &mut Stops, offset: f64, color: [u8; 4]) {
    // Offsets outside 0..=1 throw IndexSizeError in Canvas; ignored here.
    if (0.0..=1.0).contains(&offset) {
        let i = stops.partition_point(|s| s.0 <= offset);
        stops.insert(i, (offset, color));
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinearGradient {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    stops: Stops,
}

impl LinearGradient {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self {
            x0,
            y0,
            x1,
            y1,
            stops: vec![],
        }
    }
    pub fn add_color_stop(&mut self, offset: f64, color: [u8; 4]) {
        add_stop(&mut self.stops, offset, color);
    }
    /// Gradient position of user-space point (x, y); HTML: coincident points paint nothing.
    fn t_at(&self, x: f64, y: f64) -> Option<f64> {
        let (dx, dy) = (self.x1 - self.x0, self.y1 - self.y0);
        let len2 = dx * dx + dy * dy;
        (len2 > 0.0).then(|| ((x - self.x0) * dx + (y - self.y0) * dy) / len2)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadialGradient {
    x0: f64,
    y0: f64,
    r0: f64,
    x1: f64,
    y1: f64,
    r1: f64,
    stops: Stops,
}

impl RadialGradient {
    pub fn new(x0: f64, y0: f64, r0: f64, x1: f64, y1: f64, r1: f64) -> Self {
        Self {
            x0,
            y0,
            r0,
            x1,
            y1,
            r1,
            stops: vec![],
        }
    }
    pub fn add_color_stop(&mut self, offset: f64, color: [u8; 4]) {
        add_stop(&mut self.stops, offset, color);
    }
    /// HTML two-point conical gradient: the largest ω whose circle, with r(ω) >= 0, passes
    /// through (x, y); `None` paints nothing there (also for identical circles).
    fn t_at(&self, x: f64, y: f64) -> Option<f64> {
        let (dcx, dcy, dr) = (self.x1 - self.x0, self.y1 - self.y0, self.r1 - self.r0);
        if dcx == 0.0 && dcy == 0.0 && dr == 0.0 {
            return None;
        }
        let (px, py) = (x - self.x0, y - self.y0);
        let a = dcx * dcx + dcy * dcy - dr * dr;
        let b = px * dcx + py * dcy + self.r0 * dr;
        let c = px * px + py * py - self.r0 * self.r0;
        let ok = |w: f64| (self.r0 + w * dr >= 0.0).then_some(w);
        if a == 0.0 {
            return ok(c / (2.0 * b));
        }
        let disc = b * b - a * c;
        if disc < 0.0 {
            return None;
        }
        let (w1, w2) = ((b + disc.sqrt()) / a, (b - disc.sqrt()) / a);
        ok(w1.max(w2)).or_else(|| ok(w1.min(w2)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repetition {
    Repeat,
    NoRepeat,
}

impl Repetition {
    /// paintdotjs only uses "repeat"; repeat-x/repeat-y are not supported.
    pub fn from_css(s: &str) -> Self {
        if s == "no-repeat" {
            Repetition::NoRepeat
        } else {
            Repetition::Repeat
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    /// NoRepeat images carry a 1px transparent border, so `SpreadMode::Pad` spreads
    /// transparency outside the image (Skia's decal tiling).
    pixmap: Pixmap,
    repetition: Repetition,
}

impl Pattern {
    pub fn new(src: &Canvas, repetition: Repetition) -> Self {
        let pixmap = match repetition {
            Repetition::Repeat => src.pixmap.clone(),
            Repetition::NoRepeat => {
                let mut p = Pixmap::new(src.width() + 2, src.height() + 2).unwrap();
                p.draw_pixmap(
                    1,
                    1,
                    src.pixmap.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    None,
                );
                p
            }
        };
        Self { pixmap, repetition }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Style {
    /// Unpremultiplied RGBA.
    Color([u8; 4]),
    Linear(LinearGradient),
    Radial(RadialGradient),
    Pattern(Pattern),
}

impl Style {
    /// Parses the CSS colors paintdotjs passes to fillStyle/strokeStyle:
    /// #rgb, #rgba, #rrggbb, #rrggbbaa, rgb(), rgba(), and the keyword set used in the JS
    /// (`black`, `white`, `transparent`).
    pub fn parse_css(s: &str) -> Style {
        let s = s.trim();
        let hex = |h: &str| u8::from_str_radix(h, 16).unwrap_or(0);
        let c = if let Some(h) = s.strip_prefix('#').filter(|h| h.is_ascii()) {
            match h.len() {
                3 | 4 => {
                    let d: Vec<u8> = h.chars().map(|c| hex(&c.to_string()) * 17).collect();
                    [d[0], d[1], d[2], if d.len() == 4 { d[3] } else { 255 }]
                }
                6 | 8 => [
                    hex(&h[0..2]),
                    hex(&h[2..4]),
                    hex(&h[4..6]),
                    if h.len() == 8 { hex(&h[6..8]) } else { 255 },
                ],
                _ => [0, 0, 0, 255],
            }
        } else if let Some(inner) = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb(")) {
            let parts: Vec<f64> = inner
                .trim_end_matches(')')
                .split(',')
                .map(|p| p.trim().parse().unwrap_or(0.0))
                .collect();
            let part = |k, absent| parts.get(k).copied().unwrap_or(absent);
            [
                part(0, 0.0) as u8,
                part(1, 0.0) as u8,
                part(2, 0.0) as u8,
                crate::js::math_round(part(3, 1.0) * 255.0) as u8,
            ]
        } else {
            match s {
                "white" => [255, 255, 255, 255],
                "transparent" => [0, 0, 0, 0],
                _ => [0, 0, 0, 255],
            }
        };
        Style::Color(c)
    }
}

#[derive(Clone)]
struct State {
    transform: [f64; 6],
    global_alpha: f64,
    composite: CompositeOp,
    fill: Style,
    stroke: Style,
    line_width: f64,
    line_cap: LineCap,
    line_join: LineJoin,
    miter_limit: f64,
    dash: Vec<f64>,
    dash_offset: f64,
    smoothing: bool,
    smoothing_quality: SmoothingQuality,
    shadow: Shadow,
    filter_blur: Option<f64>,
    font: Font,
    text_align: TextAlign,
    text_baseline: TextBaseline,
    clip: Option<Mask>,
}

/// drawImage sampling (see `sampling`).
#[derive(Clone, Copy)]
enum Sampler {
    Skia(FilterQuality),
    Legacy,
    Mip(f64),
}

/// What a fill or stroke paints with, captured before `draw_with` borrows the canvas.
struct Source {
    style: Style,
    ctm: [f64; 6],
    quality: FilterQuality,
}

impl Source {
    fn ctm(&self, device: Transform) -> Transform {
        let m = self.ctm.map(|v| v as f32);
        Transform::from_row(m[0], m[1], m[2], m[3], m[4], m[5]).post_concat(device)
    }

    /// Fills `path` (`t` maps it onto `pm`). Gradients are evaluated per pixel with Skia's
    /// dither into a device-space pixmap first; tiny-skia does not dither.
    #[allow(clippy::too_many_arguments)]
    fn fill(
        &self,
        pm: &mut Pixmap,
        path: &tiny_skia::Path,
        t: Transform,
        rule: SkFillRule,
        blend: BlendMode,
        mask: Option<&Mask>,
        device: Transform,
    ) {
        let Some(path) = path.clone().transform(t) else {
            return;
        };
        let pixels;
        let shader = match &self.style {
            Style::Linear(_) | Style::Radial(_) => {
                let (stops, t_at): (&Stops, &dyn Fn(f64, f64) -> Option<f64>) = match &self.style {
                    Style::Linear(g) => (&g.stops, &|x, y| g.t_at(x, y)),
                    Style::Radial(g) => (&g.stops, &|x, y| g.t_at(x, y)),
                    _ => unreachable!(),
                };
                let mut m = self.ctm;
                (m[4], m[5]) = (m[4] + device.tx as f64, m[5] + device.ty as f64);
                let (Some(inv), Some(bbox)) =
                    (sampling::invert(&m), pixel_bounds(path.bounds(), pm))
                else {
                    return;
                };
                let (ox, oy) = (device.tx as i32, device.ty as i32);
                pixels = sampling::resample(&inv, bbox, |x, y, u, v| {
                    t_at(u, v).map_or([0; 4], |t| {
                        sampling::dither(sampling::gradient_color(stops, t), x - ox, y - oy)
                    })
                });
                pixels_shader(&pixels, bbox)
            }
            style => Canvas::shader_for(style, self.ctm(device), self.quality),
        };
        let paint = Paint {
            shader,
            blend_mode: blend,
            ..Paint::default()
        };
        pm.fill_path(&path, &paint, rule, Transform::identity(), mask);
    }
}

/// The rectangle `path` is, when it is one axis-aligned rectangle (like `SkPath::isRect`).
fn as_rect(path: &tiny_skia::Path) -> Option<Rect> {
    use tiny_skia::PathSegment as S;
    let b = path.bounds();
    let mut pts = vec![];
    for (i, seg) in path.segments().enumerate() {
        match (i, seg) {
            (0, S::MoveTo(p)) | (1..=4, S::LineTo(p)) => pts.push(p),
            (4 | 5, S::Close) => {}
            _ => return None,
        }
    }
    let corner = |p: &tiny_skia::Point| {
        (p.x == b.left() || p.x == b.right()) && (p.y == b.top() || p.y == b.bottom())
    };
    let edges_ok = pts
        .windows(2)
        .all(|w| (w[0].x == w[1].x) != (w[0].y == w[1].y));
    let closed = path.segments().last() == Some(S::Close);
    // The first four points must be the four corners (rejects zero-area traces such as
    // M0,0 L10,0 L10,10 L10,0 Z); a fifth may only return to the start.
    let distinct = pts.len() >= 4
        && (0..4).all(|i| (0..i).all(|j| pts[i] != pts[j]))
        && pts.get(4).is_none_or(|p| *p == pts[0]);
    (closed
        && distinct
        && edges_ok
        && pts.iter().all(corner)
        && b.width() > 0.0
        && b.height() > 0.0)
        .then_some(b)
}

/// The bounding box of `a` and `b`.
fn union(a: Option<IntRect>, b: Option<IntRect>) -> Option<IntRect> {
    match (a, b) {
        (Some(a), Some(b)) => IntRect::from_ltrb(
            a.left().min(b.left()),
            a.top().min(b.top()),
            a.right().max(b.right()),
            a.bottom().max(b.bottom()),
        ),
        (a, b) => a.or(b),
    }
}

/// `b` rounded out to pixels and clipped to `pm`; `None` when nothing is left.
fn pixel_bounds(b: Rect, pm: &Pixmap) -> Option<IntRect> {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    IntRect::from_ltrb(
        b.left().floor().clamp(0.0, w) as i32,
        b.top().floor().clamp(0.0, h) as i32,
        b.right().ceil().clamp(0.0, w) as i32,
        b.bottom().ceil().clamp(0.0, h) as i32,
    )
}

/// Paints the device-space pixels of `px`, which cover `bbox`, 1:1 (an integer translate
/// samples nearest in tiny-skia).
fn pixels_shader(px: &Pixmap, bbox: IntRect) -> Shader<'_> {
    tiny_skia::Pattern::new(
        px.as_ref(),
        SpreadMode::Pad,
        FilterQuality::Nearest,
        1.0,
        Transform::from_translate(bbox.x() as f32, bbox.y() as f32),
    )
}

#[derive(Clone, Copy, Default)]
struct Shadow {
    /// Unpremultiplied RGBA.
    color: [u8; 4],
    blur: f64,
    offset_x: f64,
    offset_y: f64,
}

impl Shadow {
    /// HTML: shadows are drawn when the color is not transparent and blur or an offset is non-zero.
    fn visible(&self) -> bool {
        self.color[3] > 0 && (self.blur > 0.0 || self.offset_x != 0.0 || self.offset_y != 0.0)
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            global_alpha: 1.0,
            composite: CompositeOp::SourceOver,
            fill: Style::Color([0, 0, 0, 255]),
            stroke: Style::Color([0, 0, 0, 255]),
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            dash: vec![],
            dash_offset: 0.0,
            smoothing: true,
            smoothing_quality: SmoothingQuality::Low,
            shadow: Shadow::default(),
            filter_blur: None,
            font: Font::default(),
            text_align: TextAlign::Start,
            text_baseline: TextBaseline::Alphabetic,
            clip: None,
        }
    }
}

#[derive(Clone)]
pub struct Canvas {
    pixmap: Pixmap,
    state: State,
    stack: Vec<State>,
    path: Path2D,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            pixmap: Pixmap::new(width.max(1), height.max(1)).expect("canvas size"),
            state: State::default(),
            stack: vec![],
            path: Path2D::new(),
        }
    }
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }
    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }
    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }
    pub fn pixmap_mut(&mut self) -> &mut Pixmap {
        &mut self.pixmap
    }

    // --- state ---
    pub fn save(&mut self) {
        self.stack.push(self.state.clone());
    }
    pub fn restore(&mut self) {
        if let Some(s) = self.stack.pop() {
            self.state = s;
        }
    }
    pub fn set_global_alpha(&mut self, a: f64) {
        if (0.0..=1.0).contains(&a) {
            self.state.global_alpha = a;
        }
    }
    pub fn set_composite(&mut self, op: CompositeOp) {
        self.state.composite = op;
    }
    pub fn set_fill_style(&mut self, s: Style) {
        self.state.fill = s;
    }
    pub fn set_stroke_style(&mut self, s: Style) {
        self.state.stroke = s;
    }
    pub fn set_line_width(&mut self, w: f64) {
        if w > 0.0 && w.is_finite() {
            self.state.line_width = w;
        }
    }
    pub fn set_line_cap(&mut self, c: LineCap) {
        self.state.line_cap = c;
    }
    pub fn set_line_join(&mut self, j: LineJoin) {
        self.state.line_join = j;
    }
    pub fn set_miter_limit(&mut self, m: f64) {
        if m > 0.0 {
            self.state.miter_limit = m;
        }
    }
    pub fn set_line_dash(&mut self, mut d: Vec<f64>) {
        if d.iter().any(|v| *v < 0.0 || !v.is_finite()) {
            return;
        }
        if d.len() % 2 == 1 {
            d.extend(d.clone());
        }
        self.state.dash = d;
    }
    pub fn set_line_dash_offset(&mut self, o: f64) {
        self.state.dash_offset = o;
    }
    pub fn set_image_smoothing(&mut self, on: bool) {
        self.state.smoothing = on;
    }
    pub fn set_image_smoothing_quality(&mut self, q: SmoothingQuality) {
        self.state.smoothing_quality = q;
    }
    /// shadowColor (unpremultiplied RGBA), shadowBlur, shadowOffsetX/Y in one call. Invalid
    /// values (negative or non-finite blur, non-finite offsets) keep the previous ones.
    pub fn set_shadow(&mut self, color: [u8; 4], blur: f64, offset_x: f64, offset_y: f64) {
        let s = &mut self.state.shadow;
        s.color = color;
        if blur >= 0.0 && blur.is_finite() {
            s.blur = blur;
        }
        if offset_x.is_finite() && offset_y.is_finite() {
            (s.offset_x, s.offset_y) = (offset_x, offset_y);
        }
    }
    /// `filter = blur(σpx)` (`Some(σ)`) or `"none"` (`None`).
    pub fn set_filter_blur(&mut self, sigma: Option<f64>) {
        self.state.filter_blur = sigma;
    }
    pub fn set_font(&mut self, font: Font) {
        self.state.font = font;
    }
    pub fn set_text_align(&mut self, a: TextAlign) {
        self.state.text_align = a;
    }
    pub fn set_text_baseline(&mut self, b: TextBaseline) {
        self.state.text_baseline = b;
    }

    // --- transform (DOMMatrix math in f64) ---
    pub fn get_transform(&self) -> [f64; 6] {
        self.state.transform
    }
    pub fn set_transform(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        self.state.transform = [a, b, c, d, e, f];
    }
    pub fn reset_transform(&mut self) {
        self.state.transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    }
    pub fn transform(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        let m = self.state.transform;
        self.state.transform = [
            m[0] * a + m[2] * b,
            m[1] * a + m[3] * b,
            m[0] * c + m[2] * d,
            m[1] * c + m[3] * d,
            m[0] * e + m[2] * f + m[4],
            m[1] * e + m[3] * f + m[5],
        ];
    }
    pub fn translate(&mut self, x: f64, y: f64) {
        self.transform(1.0, 0.0, 0.0, 1.0, x, y);
    }
    pub fn scale(&mut self, x: f64, y: f64) {
        self.transform(x, 0.0, 0.0, y, 0.0, 0.0);
    }
    pub fn rotate(&mut self, rad: f64) {
        let (s, c) = rad.sin_cos();
        self.transform(c, s, -s, c, 0.0, 0.0);
    }

    // --- path building: points are mapped through the CTM when added ---
    pub fn begin_path(&mut self) {
        self.path = Path2D::new();
    }
    fn map(&self, x: f64, y: f64) -> (f64, f64) {
        Path2D::map(&self.state.transform, x, y)
    }
    pub fn move_to(&mut self, x: f64, y: f64) {
        let p = self.map(x, y);
        self.path.move_to(p.0, p.1);
    }
    pub fn line_to(&mut self, x: f64, y: f64) {
        let p = self.map(x, y);
        self.path.line_to(p.0, p.1);
    }
    pub fn close_path(&mut self) {
        self.path.close_path();
    }
    pub fn quadratic_curve_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        let (c, e) = (self.map(cx, cy), self.map(x, y));
        self.path.quadratic_curve_to(c.0, c.1, e.0, e.1);
    }
    pub fn bezier_curve_to(&mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) {
        let (a, b, e) = (self.map(c1x, c1y), self.map(c2x, c2y), self.map(x, y));
        self.path.bezier_curve_to(a.0, a.1, b.0, b.1, e.0, e.1);
    }
    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let mut p = Path2D::new();
        p.rect(x, y, w, h);
        self.path.push_mapped(&p, &self.state.transform);
    }
    pub fn arc(&mut self, x: f64, y: f64, r: f64, start: f64, end: f64, ccw: bool) {
        self.ellipse(x, y, r, r, 0.0, start, end, ccw);
    }
    #[allow(clippy::too_many_arguments)] // Canvas ellipse() signature
    pub fn ellipse(
        &mut self,
        x: f64,
        y: f64,
        rx: f64,
        ry: f64,
        rot: f64,
        start: f64,
        end: f64,
        ccw: bool,
    ) {
        // Build in user space, then map, so the CTM applies to the curve (incl. non-uniform scale).
        let mut p = Path2D::new();
        p.ellipse(x, y, rx, ry, rot, start, end, ccw);
        self.path.push_mapped_continuing(&p, &self.state.transform);
    }
    pub fn round_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        let mut p = Path2D::new();
        p.round_rect(x, y, w, h, r);
        self.path.push_mapped(&p, &self.state.transform);
    }

    // --- painting ---
    /// Pattern sampling: smoothing off → nearest, "low" → bilinear, "medium"/"high" → bicubic.
    /// (drawImage matches Chromium's samplers instead; see `sampling`.)
    /// ponytail: transformed repeat patterns get tiny-skia's float bilinear, not Skia's legacy
    /// 4-bit one; paintdotjs fills its checkerboard/hatch patterns untransformed.
    fn filter_quality(&self) -> FilterQuality {
        match (self.state.smoothing, self.state.smoothing_quality) {
            (false, _) => FilterQuality::Nearest,
            (true, SmoothingQuality::Low) => FilterQuality::Bilinear,
            (true, _) => FilterQuality::Bicubic,
        }
    }

    /// Shader for a color or pattern. Shader coordinates are user space mapped by `ts`;
    /// tiny-skia then appends the draw call's own transform. Gradients go through `Source::fill`.
    fn shader_for(style: &Style, ts: Transform, quality: FilterQuality) -> Shader<'_> {
        match style {
            Style::Color([r, g, b, a]) => Shader::SolidColor(Color::from_rgba8(*r, *g, *b, *a)),
            Style::Pattern(p) => {
                let (spread, ts) = match p.repetition {
                    Repetition::Repeat => (SpreadMode::Repeat, ts),
                    Repetition::NoRepeat => (SpreadMode::Pad, ts.pre_translate(-1.0, -1.0)),
                };
                tiny_skia::Pattern::new(p.pixmap.as_ref(), spread, quality, 1.0, ts)
            }
            Style::Linear(_) | Style::Radial(_) => unreachable!("gradients use Source::fill"),
        }
    }

    fn needs_layer(&self) -> bool {
        self.state.global_alpha < 1.0 || self.state.composite.uncontained()
    }

    /// The HTML drawing model: `draw` renders the shape into a transparent layer A, the blur
    /// filter turns it into B, B's shadow and then B are composited with globalAlpha, the
    /// operator and the clip. Blurs are Skia's raster blur; shadows use σ = shadowBlur / 2 and
    /// ignore the CTM for their offset. Without globalAlpha, an uncontained operator, a shadow or
    /// a filter, `draw` paints straight onto the canvas.
    ///
    /// `bounds`: device bounds of what `draw` paints. Layers cover only those grown by the blur
    /// reach and cut to the canvas region they can reach; the shadow gets its own layer. `draw`
    /// may run twice (shadow, then shape); `draw(pixmap, blend, clip, device)`, where `device`
    /// maps canvas pixels to `pixmap` pixels.
    ///
    /// `rect`: set when the shape is exactly this device-space rectangle filled with a color of
    /// alpha `.1` (0..=1). Chromium blurs such shadows with a Skia blur mask filter, which takes
    /// the analytic `BlurRect` path; the shadow respects the source alpha (WPT 2d.shadow.alpha.5).
    ///
    /// Uncontained operators composite every pass as a full-canvas layer, as Blink's
    /// `CompositedDraw` does: pixels outside a pass see a transparent source.
    fn draw_with(
        &mut self,
        bounds: Option<Rect>,
        rect: Option<(Rect, f32)>,
        draw: impl Fn(&mut Pixmap, BlendMode, Option<&Mask>, Transform),
    ) {
        let clip = self.state.clip.clone();
        let blend = self.state.composite.skia();
        let filter = self.state.filter_blur.filter(|s| *s > 0.0);
        let shadow = Some(self.state.shadow).filter(Shadow::visible);
        if !self.needs_layer() && filter.is_none() && shadow.is_none() {
            draw(
                &mut self.pixmap,
                blend,
                clip.as_ref(),
                Transform::identity(),
            );
            return;
        }
        let (cw, ch) = (self.pixmap.width(), self.pixmap.height());
        let canvas = IntRect::from_xywh(0, 0, cw, ch).unwrap();
        let uncontained = self.state.composite.uncontained();
        let Some(shape) = bounds.and_then(|b| b.round_out()?.make_outset(1, 1)) else {
            return;
        };
        let rf = filter.map_or(0, |s| blur::skia_blur_extent(s) as i32);
        // Layer A over `area`, blurred by the filter into B.
        let layer = |area: IntRect| {
            let mut px = Pixmap::new(area.width(), area.height())?;
            let device = Transform::from_translate(-area.x() as f32, -area.y() as f32);
            draw(&mut px, BlendMode::SourceOver, None, device);
            if let Some(sigma) = filter {
                blur::skia_blur_premul(px.data_mut(), area.width(), area.height(), sigma);
            }
            Some(px)
        };
        // The blurs treat everything outside a layer as transparent, so a layer needs `target`
        // (where results land) within `reach` of the shape, and the shape within `reach` of it.
        let span = |target: IntRect, reach: i32| {
            let near = |a: IntRect, b: IntRect| a.intersect(&b.make_outset(reach, reach)?);
            union(near(target, shape), near(shape, target))
        };
        let paint = PixmapPaint {
            opacity: self.state.global_alpha as f32,
            blend_mode: blend,
            quality: FilterQuality::Nearest,
        };
        // Composites one pass, drawn at `t` with `opacity`; uncontained operators see it over the
        // whole canvas (Blink's CompositedDraw gives each pass its own full-canvas layer).
        let composite = |canvas: &mut Pixmap, px: Option<&Pixmap>, t: Transform, opacity: f32| {
            let paint = PixmapPaint { opacity, ..paint };
            if !uncontained {
                if let Some(px) = px {
                    canvas.draw_pixmap(0, 0, px.as_ref(), &paint, t, clip.as_ref());
                }
                return;
            }
            let Some(mut full) = Pixmap::new(cw, ch) else {
                return;
            };
            if let Some(px) = px {
                let copy = PixmapPaint::default();
                full.draw_pixmap(0, 0, px.as_ref(), &copy, t, None);
            }
            let id = Transform::identity();
            canvas.draw_pixmap(0, 0, full.as_ref(), &paint, id, clip.as_ref());
        };
        let ga = self.state.global_alpha as f32;
        // Blink draws rect shadows through the blur mask filter in both its direct and its
        // composited (uncontained operator) paths; Skia's filterRectsToNine decides BlurRect.
        let rect_shadow = shadow.zip(rect).filter(|(s, (r, _))| {
            let sigma = (s.blur / 2.0).min(blur::MAX_SIGMA) as f32;
            let edges = [r.left(), r.top(), r.right(), r.bottom()];
            filter.is_none() && s.blur > 0.0 && blur::skia_blur_rect_fits(sigma, edges)
        });
        if let Some((s, (r, src_alpha))) = rect_shadow {
            let (ox, oy) = (s.offset_x as f32, s.offset_y as f32);
            let sigma = (s.blur / 2.0).min(blur::MAX_SIGMA) as f32;
            let moved = [r.left() + ox, r.top() + oy, r.right() + ox, r.bottom() + oy];
            let clip_area = [0, 0, cw as i32, ch as i32];
            let pass = blur::skia_blur_rect_mask(sigma, moved, clip_area).map(|(a, mask)| {
                let mut sh = Pixmap::new(a[2] as u32, a[3] as u32).unwrap();
                let c = s.color.map(|v| v as f32 / 255.0);
                // The shadow paint's alpha folds in the source color's alpha and globalAlpha,
                // so the pass itself composites at full opacity.
                let alpha = c[3] * src_alpha * ga;
                for (px, m) in sh.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(&mask) {
                    let a = *m as f32 * alpha;
                    *px = [a * c[0], a * c[1], a * c[2], a].map(|v| v.round() as u8);
                }
                (a, sh)
            });
            match pass {
                Some(([ax, ay, ..], sh)) => {
                    let t = Transform::from_translate(ax as f32, ay as f32);
                    composite(&mut self.pixmap, Some(&sh), t, 1.0);
                }
                None => composite(&mut self.pixmap, None, Transform::identity(), 1.0),
            }
        } else if let Some(s) = shadow {
            // ponytail: Skia blurs other shapes' (and small rects') coverage masks with
            // SkMaskBlurFilter (GaussPass math at σ >= 2, an SkGaussFilter kernel below) and
            // multiplies the source alpha in afterwards; this blurs the drawn layer instead.
            let sigma = s.blur / 2.0;
            let reach = rf + blur::skia_blur_extent(sigma) as i32;
            // The shape region whose shadow lands on the canvas: the canvas moved back by the
            // offset (`as` saturates huge offsets).
            let (ox, oy) = (s.offset_x, s.offset_y);
            let source = IntRect::from_ltrb(
                (-ox).floor() as i32,
                (-oy).floor() as i32,
                (cw as f64 - ox).ceil() as i32,
                (ch as f64 - oy).ceil() as i32,
            );
            // B is also needed between the shadow source and the shape.
            let area = source.and_then(|src| {
                let between = src
                    .make_outset(reach - rf, reach - rf)
                    .and_then(|r| r.intersect(&shape.make_outset(rf, rf)?));
                union(span(src, reach), between)
            });
            let pass = area.and_then(|r| Some((r, layer(r)?)));
            if let Some((rect, mut sh)) = pass {
                blur::skia_blur_premul(sh.data_mut(), rect.width(), rect.height(), sigma);
                // Skia's drop shadow: shadow color SrcIn the blurred layer.
                let c = s.color.map(|v| v as f32 / 255.0);
                for px in sh.data_mut().as_chunks_mut::<4>().0 {
                    let a = px[3] as f32 * c[3];
                    for (i, v) in px.iter_mut().enumerate() {
                        *v = (a * if i == 3 { 1.0 } else { c[i] }).round() as u8;
                    }
                }
                // ponytail: tiny-skia samples translate-only pixmaps with nearest, so fractional
                // offsets snap to whole pixels (paintdotjs only uses 0); resample if one is needed.
                let t = Transform::from_translate(
                    (rect.x() as f64 + ox) as f32,
                    (rect.y() as f64 + oy) as f32,
                );
                composite(&mut self.pixmap, Some(&sh), t, ga);
            } else {
                composite(&mut self.pixmap, None, Transform::identity(), ga);
            }
        }
        match span(canvas, rf).and_then(|r| Some((r, layer(r)?))) {
            Some((rect, b)) => {
                let t = Transform::from_translate(rect.x() as f32, rect.y() as f32);
                composite(&mut self.pixmap, Some(&b), t, ga);
            }
            None => composite(&mut self.pixmap, None, Transform::identity(), ga),
        }
    }

    /// Device bounds of user-space rect `r` grown by `grow` user units on every side.
    fn device_bounds(&self, r: Rect, grow: f64) -> Option<Rect> {
        let m = &self.state.transform;
        let (l, t, rt, b) = (
            r.left() as f64 - grow,
            r.top() as f64 - grow,
            r.right() as f64 + grow,
            r.bottom() as f64 + grow,
        );
        let pts = [(l, t), (rt, t), (l, b), (rt, b)].map(|(x, y)| Path2D::map(m, x, y));
        let xs = pts.map(|p| p.0 as f32);
        let ys = pts.map(|p| p.1 as f32);
        let min = |v: [f32; 4]| v.into_iter().fold(f32::INFINITY, f32::min);
        let max = |v: [f32; 4]| v.into_iter().fold(f32::NEG_INFINITY, f32::max);
        Rect::from_ltrb(min(xs), min(ys), max(xs), max(ys))
    }

    pub fn fill_path2d(&mut self, user_path: &Path2D, rule: FillRule) {
        let mut mapped = Path2D::new();
        mapped.push_mapped(user_path, &self.state.transform);
        self.fill_mapped(&mapped, rule);
    }

    fn source(&self, style: &Style) -> Source {
        Source {
            style: style.clone(),
            ctm: self.state.transform,
            quality: self.filter_quality(),
        }
    }

    fn fill_mapped(&mut self, path: &Path2D, rule: FillRule) {
        let Some(sk) = path.to_skia() else { return };
        let src = self.source(&self.state.fill);
        self.draw_with(
            Some(sk.bounds()),
            match self.state.fill {
                Style::Color([.., a]) => as_rect(&sk).map(|r| (r, a as f32 / 255.0)),
                _ => None,
            },
            |pm, blend, mask, device| {
                src.fill(pm, &sk, device, rule.skia(), blend, mask, device);
            },
        );
    }

    pub fn fill(&mut self, rule: FillRule) {
        let path = self.path.clone();
        self.fill_mapped(&path, rule);
    }

    fn sk_stroke(&self) -> Stroke {
        // Stroke geometry is computed in user space and transformed, like Skia does for canvas.
        let mut s = Stroke {
            width: self.state.line_width as f32,
            miter_limit: self.state.miter_limit as f32,
            line_cap: match self.state.line_cap {
                LineCap::Butt => tiny_skia::LineCap::Butt,
                LineCap::Round => tiny_skia::LineCap::Round,
                LineCap::Square => tiny_skia::LineCap::Square,
            },
            line_join: match self.state.line_join {
                LineJoin::Miter => tiny_skia::LineJoin::Miter,
                LineJoin::Round => tiny_skia::LineJoin::Round,
                LineJoin::Bevel => tiny_skia::LineJoin::Bevel,
            },
            dash: None,
        };
        if !self.state.dash.is_empty() {
            s.dash = StrokeDash::new(
                self.state.dash.iter().map(|v| *v as f32).collect(),
                self.state.dash_offset as f32,
            );
        }
        s
    }

    fn stroke_mapped(&mut self, device_path: &Path2D) {
        // Strokes must be built in user space: un-map the device path through the inverse CTM.
        let Some(inv) = self.inverse_transform() else {
            return;
        };
        let mut user = Path2D::new();
        user.push_mapped(device_path, &inv);
        let Some(sk) = user.to_skia() else { return };
        let stroke = self.sk_stroke();
        let src = self.source(&self.state.stroke);
        // Joins reach miterLimit · lineWidth / 2 from the path, square caps √2 · lineWidth / 2.
        let miter = match self.state.line_join {
            LineJoin::Miter => self.state.miter_limit.max(std::f64::consts::SQRT_2),
            _ => std::f64::consts::SQRT_2,
        };
        let bounds = self.device_bounds(sk.bounds(), self.state.line_width / 2.0 * miter);
        self.draw_with(bounds, None, |pm, blend, mask, device| {
            let t = src.ctm(device);
            if let Style::Linear(_) | Style::Radial(_) = src.style {
                // ponytail: gradient strokes skip tiny-skia's thin-stroke hairline mode.
                let scale = tiny_skia::PathStroker::compute_resolution_scale(&t);
                if let Some(outline) = sk.stroke(&stroke, scale) {
                    src.fill(pm, &outline, t, SkFillRule::Winding, blend, mask, device);
                }
                return;
            }
            let p = Paint {
                shader: Self::shader_for(&src.style, Transform::identity(), src.quality),
                blend_mode: blend,
                ..Paint::default()
            };
            pm.stroke_path(&sk, &p, &stroke, t, mask);
        });
    }

    pub fn stroke(&mut self) {
        let path = self.path.clone();
        self.stroke_mapped(&path);
    }

    pub fn stroke_path2d(&mut self, user_path: &Path2D) {
        let mut mapped = Path2D::new();
        mapped.push_mapped(user_path, &self.state.transform);
        self.stroke_mapped(&mapped);
    }

    pub fn clip(&mut self, rule: FillRule) {
        let Some(sk) = self.path.to_skia() else {
            // Empty path clips everything.
            self.state.clip = Some(Mask::new(self.pixmap.width(), self.pixmap.height()).unwrap());
            return;
        };
        let mut mask = Mask::new(self.pixmap.width(), self.pixmap.height()).unwrap();
        // Chromium antialiases 2D canvas clips (pinned by the `clip-then-fill` case: 128 on a .5 edge).
        mask.fill_path(&sk, rule.skia(), true, Transform::identity());
        if let Some(prev) = &self.state.clip {
            let prev_data = prev.data().to_vec();
            for (d, p) in mask.data_mut().iter_mut().zip(prev_data) {
                *d = ((*d as u32 * p as u32 + 127) / 255) as u8;
            }
        }
        self.state.clip = Some(mask);
    }

    /// Chromium semantics: (x, y) is a device-space point, not mapped through the CTM. It is
    /// un-mapped through the inverse CTM and tested exactly against the user-space path
    /// (`SkPath::contains`); boundary points count as inside (canvas-is-point-in-path).
    pub fn is_point_in_path(&self, user_path: &Path2D, x: f64, y: f64, rule: FillRule) -> bool {
        if !x.is_finite() || !y.is_finite() {
            return false;
        }
        let Some(inv) = self.inverse_transform() else {
            return false;
        };
        let p = Path2D::map(&inv, x as f32 as f64, y as f32 as f64);
        user_path.contains(p.0 as f32, p.1 as f32, rule)
    }

    fn inverse_transform(&self) -> Option<[f64; 6]> {
        sampling::invert(&self.state.transform)
    }

    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let mut p = Path2D::new();
        p.rect(x, y, w, h);
        self.fill_path2d(&p, FillRule::NonZero);
    }

    pub fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let mut p = Path2D::new();
        p.rect(x, y, w, h);
        // Skia draws a thin (<= 1 device px) axis-aligned miter-joined rect stroke with
        // `SkScan::AntiFrameRect`: exact ring coverage, corners included. tiny-skia would use
        // its hairline mode, which leaves the corner pixels empty, so fill the ring instead.
        let t = self.state.transform;
        let lw = self.state.line_width;
        if self.state.dash.is_empty()
            && self.state.line_join == LineJoin::Miter
            && self.state.miter_limit >= std::f64::consts::SQRT_2
            && t[1] == 0.0
            && t[2] == 0.0
            && w > lw
            && h > lw
            && lw * t[0].abs().min(t[3].abs()) <= 1.0
        {
            let m = lw / 2.0;
            let mut ring = Path2D::new();
            ring.rect(x + m, y + m, w - lw, h - lw);
            ring.rect(x - m, y - m, w + lw, h + lw);
            self.save();
            self.state.fill = self.state.stroke.clone();
            self.fill_path2d(&ring, FillRule::EvenOdd);
            self.restore();
            return;
        }
        self.stroke_path2d(&p);
    }

    pub fn clear_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let mut p = Path2D::new();
        p.rect(x, y, w, h);
        let mut mapped = Path2D::new();
        mapped.push_mapped(&p, &self.state.transform);
        let Some(sk) = mapped.to_skia() else { return };
        let paint = Paint {
            blend_mode: BlendMode::Clear,
            anti_alias: true,
            ..Paint::default()
        };
        let clip = self.state.clip.clone();
        self.pixmap.fill_path(
            &sk,
            &paint,
            SkFillRule::Winding,
            Transform::identity(),
            clip.as_ref(),
        );
    }

    // --- drawImage ---
    pub fn draw_image(&mut self, src: &Canvas, dx: f64, dy: f64) {
        let (w, h) = (src.width() as f64, src.height() as f64);
        self.draw_image_sub(src, 0.0, 0.0, w, h, dx, dy, w, h);
    }

    pub fn draw_image_scaled(&mut self, src: &Canvas, dx: f64, dy: f64, dw: f64, dh: f64) {
        let (w, h) = (src.width() as f64, src.height() as f64);
        self.draw_image_sub(src, 0.0, 0.0, w, h, dx, dy, dw, dh);
    }

    /// HTML drawImage: negative sizes are normalized, the source rect is clipped to the image
    /// (shrinking the destination to match), and sampling stays inside the source rect
    /// (Blink's clamp-to-source-rect). Smoothing samplers: see `sampling`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image_sub(
        &mut self,
        src: &Canvas,
        sx: f64,
        sy: f64,
        sw: f64,
        sh: f64,
        dx: f64,
        dy: f64,
        dw: f64,
        dh: f64,
    ) {
        if ![sx, sy, sw, sh, dx, dy, dw, dh]
            .iter()
            .all(|v| v.is_finite())
        {
            return;
        }
        let norm = |p: f64, len: f64| if len < 0.0 { (p + len, -len) } else { (p, len) };
        let ((sx, sw), (sy, sh)) = (norm(sx, sw), norm(sy, sh));
        let ((dx, dw), (dy, dh)) = (norm(dx, dw), norm(dy, dh));
        let (iw, ih) = (src.width() as f64, src.height() as f64);
        let (x0, y0) = (sx.max(0.0), sy.max(0.0));
        let (x1, y1) = ((sx + sw).min(iw), (sy + sh).min(ih));
        if !(x1 > x0 && y1 > y0 && dw > 0.0 && dh > 0.0) {
            return;
        }
        let (kx, ky) = (dw / sw, dh / sh);
        let (dx, dy) = (dx + (x0 - sx) * kx, dy + (y0 - sy) * ky);
        // Crop to the pixel-aligned source rect so clamped sampling stays inside it.
        let (cx, cy) = (x0.floor(), y0.floor());
        let crop =
            IntRect::from_ltrb(cx as i32, cy as i32, x1.ceil() as i32, y1.ceil() as i32).unwrap();
        let img = if crop.width() == src.width() && crop.height() == src.height() {
            Cow::Borrowed(&src.pixmap)
        } else {
            Cow::Owned(src.pixmap.clone_rect(crop).unwrap())
        };
        let (sx, sy) = (x0 - cx, y0 - cy);
        let Some(rect) = Rect::from_xywh(sx as f32, sy as f32, (x1 - x0) as f32, (y1 - y0) as f32)
        else {
            return;
        };
        // Cropped image → device: CTM · translate(dx, dy) · scale(kx, ky) · translate(-sx, -sy).
        let c = self.state.transform;
        let m: sampling::Matrix = [
            c[0] * kx,
            c[1] * kx,
            c[2] * ky,
            c[3] * ky,
            c[0] * (dx - sx * kx) + c[2] * (dy - sy * ky) + c[4],
            c[1] * (dx - sx * kx) + c[3] * (dy - sy * ky) + c[5],
        ];
        let scale = m[0].hypot(m[1]).min(m[2].hypot(m[3]));
        let integer_translate =
            m[..4] == [1.0, 0.0, 0.0, 1.0] && m[4] % 1.0 == 0.0 && m[5] % 1.0 == 0.0;
        let sampler = match (self.state.smoothing, self.state.smoothing_quality) {
            // Like Skia: filtering at an integer translate is a copy.
            (false, _) => Sampler::Skia(FilterQuality::Nearest),
            _ if integer_translate => Sampler::Skia(FilterQuality::Nearest),
            (true, SmoothingQuality::Low) => Sampler::Legacy,
            (true, SmoothingQuality::High) if scale >= 1.0 => Sampler::Skia(FilterQuality::Bicubic),
            (true, _) => Sampler::Mip(sampling::mip_level(scale)),
        };
        let count = match sampler {
            Sampler::Mip(level) => level.ceil() as usize,
            _ => 0,
        };
        let levels = match sampler {
            Sampler::Mip(_) => sampling::mip_chain(&img, count),
            _ => vec![],
        };
        // Integer-translate layer compositing: Skia's lowp pipeline (see `lowp`).
        let int = |v: f32| v.fract() == 0.0;
        if integer_translate
            && !matches!(sampler, Sampler::Mip(_) | Sampler::Legacy)
            && lowp::supports(self.state.composite)
            && self.state.filter_blur.is_none_or(|s| s <= 0.0)
            && !Shadow::visible(&self.state.shadow)
            && [rect.x(), rect.y(), rect.width(), rect.height()]
                .into_iter()
                .all(int)
        {
            let region = (
                rect.x() as i64,
                rect.y() as i64,
                rect.width() as i64,
                rect.height() as i64,
            );
            let origin = (m[4] as i64 + region.0, m[5] as i64 + region.1);
            lowp::draw_image(
                &mut self.pixmap,
                &img,
                region,
                origin,
                self.state.global_alpha as f32,
                self.state.composite,
                self.state.clip.as_ref(),
            );
            return;
        }
        let t = Transform::from_row(
            m[0] as f32,
            m[1] as f32,
            m[2] as f32,
            m[3] as f32,
            m[4] as f32,
            m[5] as f32,
        );
        let Some(quad) = PathBuilder::from_rect(rect).transform(t) else {
            return;
        };
        self.draw_with(Some(quad.bounds()), None, |pm, blend, mask, device| {
            let t = t.post_concat(device);
            let image = |shader| Paint {
                shader,
                blend_mode: blend,
                ..Paint::default()
            };
            let Sampler::Skia(quality) = sampler else {
                // Resample the quad's device bbox ourselves, then fill the quad with it.
                let Some(quad) = quad.clone().transform(device) else {
                    return;
                };
                let Some(bbox) = pixel_bounds(quad.bounds(), pm) else {
                    return;
                };
                let mut md = m;
                (md[4], md[5]) = (md[4] + device.tx as f64, md[5] + device.ty as f64);
                let Some(inv) = sampling::invert(&md) else {
                    return;
                };
                let res = sampling::resample(&inv, bbox, |_, _, u, v| match sampler {
                    Sampler::Mip(level) => sampling::mip_linear(&levels, u, v, level),
                    _ => sampling::legacy_bilinear(&img, u, v),
                });
                let shader = pixels_shader(&res, bbox);
                pm.fill_path(
                    &quad,
                    &image(shader),
                    SkFillRule::Winding,
                    Transform::identity(),
                    mask,
                );
                return;
            };
            let shader = tiny_skia::Pattern::new(
                img.as_ref().as_ref(),
                SpreadMode::Pad,
                quality,
                1.0,
                Transform::identity(),
            );
            pm.fill_rect(rect, &image(shader), t, mask);
        });
    }

    // --- text ---
    fn text_layout(&self, text: &str, x: f64, y: f64, drawing: bool) -> text::Layout {
        let s = &self.state;
        let ctm = drawing.then_some(&s.transform);
        text::layout(text, &s.font, s.text_align, s.text_baseline, x, y, ctm)
    }

    /// Chromium draws text as glyph masks: each glyph's exact-area coverage (synthetic bold:
    /// of the outline grown by Skia's fake-bold stroke) goes through Skia's mask gamma, and the
    /// glyphs are accumulated into one coverage mask that the fill style is painted through.
    pub fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        let layout = self.text_layout(text, x, y, true);
        let m = self.state.transform;
        let ctm = Transform::from_row(
            m[0] as f32,
            m[1] as f32,
            m[2] as f32,
            m[3] as f32,
            m[4] as f32,
            m[5] as f32,
        );
        // Device-space glyph outlines (synthetic bold is already emboldened by the layout).
        let glyphs: Vec<_> = layout
            .glyphs
            .iter()
            .filter_map(|g| g.to_skia()?.transform(ctm))
            .collect();
        let bounds = glyphs.iter().map(|g| g.bounds()).reduce(|a, b| {
            Rect::from_ltrb(
                a.left().min(b.left()),
                a.top().min(b.top()),
                a.right().max(b.right()),
                a.bottom().max(b.bottom()),
            )
            .unwrap_or(a)
        });
        let Some(bounds) = bounds else { return };
        let lut = text::mask_gamma(match self.state.fill {
            // SkComputeLuminance; shaders use Skia's mid-gray stand-in.
            Style::Color([r, g, b, _]) => {
                ((r as u32 * 54 + g as u32 * 183 + b as u32 * 19) >> 8) as u8
            }
            _ => 128,
        });
        let src = self.source(&self.state.fill);
        self.draw_with(Some(bounds), None, |pm, blend, clip, device| {
            let Some(area) = bounds.transform(device).and_then(|b| pixel_bounds(b, pm)) else {
                return;
            };
            // ponytail: the mask spans the whole target because tiny-skia masks must match it.
            let Some(mut mask) = Mask::new(pm.width(), pm.height()) else {
                return;
            };
            let pw = pm.width() as usize;
            for glyph in &glyphs {
                let Some(glyph) = glyph.clone().transform(device) else {
                    continue;
                };
                // Rasterize only the visible part, so huge glyphs stay cheap.
                let vis = glyph
                    .bounds()
                    .round_out()
                    .and_then(|gb| gb.intersect(&area));
                let Some(vis) = vis else {
                    continue;
                };
                let cover = text::coverage(&glyph, vis);
                let data = mask.data_mut();
                for yy in vis.top()..vis.bottom() {
                    for xx in vis.left()..vis.right() {
                        let gi = (yy - vis.y()) as usize * vis.width() as usize
                            + (xx - vis.x()) as usize;
                        let m = lut[cover[gi] as usize] as u32;
                        let d = &mut data[yy as usize * pw + xx as usize];
                        *d = (*d as u32 + (m * (255 - *d as u32) + 127) / 255) as u8;
                    }
                }
            }
            if let Some(k) = clip {
                for (d, k) in mask.data_mut().iter_mut().zip(k.data()) {
                    *d = ((*d as u32 * *k as u32 + 127) / 255) as u8;
                }
            }
            let r = Rect::from_xywh(
                area.x() as f32,
                area.y() as f32,
                area.width() as f32,
                area.height() as f32,
            )
            .unwrap();
            let id = Transform::identity();
            let rect = PathBuilder::from_rect(r);
            src.fill(
                pm,
                &rect,
                id,
                SkFillRule::Winding,
                blend,
                Some(&mask),
                device,
            );
        });
    }

    /// Strokes each glyph outline, like Skia strokes glyph paths (synthetic bold is not applied;
    /// paintdotjs never strokes text).
    pub fn stroke_text(&mut self, text: &str, x: f64, y: f64) {
        for glyph in self.text_layout(text, x, y, true).glyphs {
            self.stroke_path2d(&glyph);
        }
    }

    pub fn measure_text(&self, text: &str) -> TextMetrics {
        self.text_layout(text, 0.0, 0.0, false).metrics
    }

    // --- ImageData ---
    pub fn get_image_data(&self, x: i32, y: i32, w: u32, h: u32) -> ImageData {
        let mut out = ImageData::new(w, h);
        let (cw, ch) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        let src = self.pixmap.data();
        for row in 0..h as i32 {
            for col in 0..w as i32 {
                let (sx, sy) = (x + col, y + row);
                if sx < 0 || sy < 0 || sx >= cw || sy >= ch {
                    continue;
                }
                let si = ((sy * cw + sx) * 4) as usize;
                let di = ((row * w as i32 + col) * 4) as usize;
                let a = src[si + 3];
                out.data[di] = unpremultiply(src[si], a);
                out.data[di + 1] = unpremultiply(src[si + 1], a);
                out.data[di + 2] = unpremultiply(src[si + 2], a);
                out.data[di + 3] = a;
            }
        }
        out
    }

    /// putImageData ignores transform, clip, globalAlpha and compositing (HTML spec).
    pub fn put_image_data(&mut self, img: &ImageData, x: i32, y: i32) {
        let (cw, ch) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        let dst = self.pixmap.data_mut();
        for row in 0..img.height as i32 {
            for col in 0..img.width as i32 {
                let (dx, dy) = (x + col, y + row);
                if dx < 0 || dy < 0 || dx >= cw || dy >= ch {
                    continue;
                }
                let si = ((row * img.width as i32 + col) * 4) as usize;
                let di = ((dy * cw + dx) * 4) as usize;
                let a = img.data[si + 3];
                dst[di] = premultiply(img.data[si], a);
                dst[di + 1] = premultiply(img.data[si + 1], a);
                dst[di + 2] = premultiply(img.data[si + 2], a);
                dst[di + 3] = a;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Final review M2: short `rgb()` and non-ASCII hex fall back instead of panicking.
    #[test]
    fn malformed_css_colors_do_not_panic() {
        assert_eq!(Style::parse_css("rgb(1,2)"), Style::Color([1, 2, 0, 255]));
        assert_eq!(Style::parse_css("rgba()"), Style::Color([0, 0, 0, 255]));
        assert_eq!(Style::parse_css("#éa"), Style::Color([0, 0, 0, 255]));
        assert_eq!(Style::parse_css("#aééx"), Style::Color([0, 0, 0, 255]));
    }

    /// Huge shadow offsets and blurs, a huge filter blur and non-finite drawImage arguments
    /// neither panic nor allocate far beyond the canvas.
    #[test]
    fn extreme_draw_arguments_stay_bounded() {
        let mut cv = Canvas::new(8, 8);
        cv.set_shadow([0, 0, 0, 255], 1e9, 1e12, -1e12);
        cv.fill_rect(-1e5, -1e5, 2e5, 2e5);
        cv.begin_path();
        cv.arc(4.0, 4.0, 3.0, 0.0, 6.0, false);
        cv.fill(FillRule::NonZero);
        cv.set_shadow([0, 0, 0, 0], 0.0, 0.0, 0.0);
        cv.set_filter_blur(Some(1e9));
        cv.fill_rect(0.0, 0.0, 4.0, 4.0);
        let src = Canvas::new(2, 2);
        cv.draw_image_sub(&src, f64::NAN, 0.0, 1.0, 1.0, 0.0, 0.0, f64::INFINITY, 1.0);
        // A 100000px glyph (and a 1000× CTM) only rasterizes the visible 8×8 pixels.
        cv.set_filter_blur(None);
        cv.set_font(Font {
            family: "Noto Sans".into(),
            size_px: 100000.0,
            bold: true,
            italic: false,
        });
        cv.fill_text("W", -50000.0, 60000.0);
        cv.save();
        cv.scale(1000.0, 1000.0);
        cv.set_font(Font::default());
        cv.fill_text("H", -2.0, 4.0);
        cv.restore();
        // The rect fill covers the canvas.
        assert_eq!(cv.pixmap().pixel(7, 7).unwrap().alpha(), 255);
    }

    #[test]
    fn as_rect_accepts_rectangles_only() {
        let path = |pts: &[(f32, f32)]| {
            let mut b = PathBuilder::new();
            b.move_to(pts[0].0, pts[0].1);
            for p in &pts[1..] {
                b.line_to(p.0, p.1);
            }
            b.close();
            b.finish().unwrap()
        };
        let rect = [(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0)];
        assert!(as_rect(&path(&rect)).is_some());
        // Zero-area trace with rectangle bounds.
        let trace = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (10.0, 0.0)];
        assert!(as_rect(&path(&trace)).is_none());
    }
}
