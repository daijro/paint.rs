//! CBDT/CBLC PNG glyphs in the shared document Text pipeline. No COLR/SVG/GSUB renderer.
//! Scalar fallback plus VS16 preference and VS15 outline preference covers the required
//! Calendar defaults. U+200B and selectors retain source units but have no ink/advance.
//! ZWJ compositions, modifier sequences, flags, keycaps, tags, bidi and general shaping are
//! unsupported; their constituent scalar glyphs are not a claim of sequence support.
//! Intrinsic bitmap RGB is preserved. Bold/italic synthesis remains outline-only; bitmap
//! scaling is portable bilinear/nearest sampling, without a native bitmap-hinting parity claim.
//! Each line retains at most 16 MiB of distinct decoded bitmap data (repeats share an Arc).
//! Missing, malformed or over-budget bitmap glyphs use the deterministic outline fallback.
use super::*;
use crate::canvas::ImageData;
use crate::effect::{
    RenderInfo,
    bb::blocks::{colour_exact, source_over},
    surface::{Alpha, Edge, EffectSurface, Space},
};
use image::{ImageDecoder, codecs::png::PngDecoder};
use std::io::Cursor;

mod cblc_guard;

// Bound untrusted font PNG headers, decoder memory and the sum retained by one line.
// These are portable safety limits, not limits advertised by the font format.
pub(super) const MAX_RUN_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_GLYPH_SIDE: u32 = 512;
const MAX_PNG_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Bitmap {
    pub image: Arc<ImageData>,
    pub origin: [f64; 2],
    pub scale: f64,
}
#[derive(Clone, Debug)]
pub enum DrawItem {
    Outline(Path),
    Bitmap(Bitmap),
}
impl DrawItem {
    pub(super) fn translate_y(self, y: f64) -> Option<Self> {
        match self {
            Self::Outline(p) => p
                .transform(Transform::from_translate(0.0, y as f32))
                .map(Self::Outline),
            Self::Bitmap(mut b) => {
                b.origin[1] += y;
                Some(Self::Bitmap(b))
            }
        }
    }
    /// The item moved by `m`; a bitmap only moves its centre (it is not turned or scaled).
    pub(super) fn transform(self, m: Transform) -> Option<Self> {
        match self {
            Self::Outline(p) => p.transform(m).map(Self::Outline),
            Self::Bitmap(mut b) => {
                let half = [
                    b.image.width as f64 * b.scale / 2.0,
                    b.image.height as f64 * b.scale / 2.0,
                ];
                let mut c = tiny_skia::Point::from_xy(
                    (b.origin[0] + half[0]) as f32,
                    (b.origin[1] + half[1]) as f32,
                );
                m.map_points(std::slice::from_mut(&mut c));
                b.origin = [c.x as f64 - half[0], c.y as f64 - half[1]];
                Some(Self::Bitmap(b))
            }
        }
    }
    pub fn bounds(&self) -> Rectangle {
        match self {
            Self::Outline(p) => {
                let b = p.bounds();
                Rectangle::new(
                    b.x() as f64,
                    b.y() as f64,
                    b.width() as f64,
                    b.height() as f64,
                )
            }
            Self::Bitmap(b) => Rectangle::new(
                b.origin[0],
                b.origin[1],
                b.image.width as f64 * b.scale,
                b.image.height as f64 * b.scale,
            ),
        }
    }
}
impl Bitmap {
    pub(super) fn from_glyph(
        f: &ttf_parser::Face<'_>,
        gid: u16,
        px: f64,
        x: f64,
        budget: &mut usize,
    ) -> Option<Self> {
        // Restrict this extension to CBDT/CBLC PNGs. Other color font formats retain fallback.
        f.tables().cbdt?;
        let ppem = px.ceil().clamp(1.0, 65535.0) as u16;
        // ttf-parser 0.25.1 adds several untrusted CBLC offsets without checked
        // arithmetic. Check the selected PNG path before asking it for the glyph.
        if !cblc_guard::safe_png_lookup(f, gid, ppem) {
            return None;
        }
        let g = f.glyph_raster_image(ttf_parser::GlyphId(gid), ppem)?;
        if g.format != ttf_parser::RasterImageFormat::PNG || g.pixels_per_em == 0 {
            return None;
        }
        let image = decode_png(g.data, [g.width as u32, g.height as u32], budget)?;
        let scale = px / g.pixels_per_em as f64;
        Some(Self {
            image: Arc::new(image),
            origin: [
                x + g.x as f64 * scale,
                -(g.y as f64 + g.height as f64) * scale,
            ],
            scale,
        })
    }
}
fn decode_png(data: &[u8], dimensions: [u32; 2], budget: &mut usize) -> Option<ImageData> {
    if data.len() > MAX_PNG_BYTES
        || dimensions.contains(&0)
        || dimensions.iter().any(|&v| v > MAX_GLYPH_SIDE)
    {
        return None;
    }
    let len = (dimensions[0] as usize)
        .checked_mul(dimensions[1] as usize)?
        .checked_mul(4)?;
    if len > *budget {
        return None;
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_GLYPH_SIDE);
    limits.max_image_height = Some(MAX_GLYPH_SIDE);
    limits.max_alloc = Some(4 * 1024 * 1024);
    // image's PNG decoder expands indexed palettes and tRNS before returning RGBA.
    let decoder = PngDecoder::with_limits(Cursor::new(data), limits).ok()?;
    if decoder.dimensions() != (dimensions[0], dimensions[1]) {
        return None;
    }
    let image = image::DynamicImage::from_decoder(decoder)
        .ok()?
        .into_rgba8();
    *budget -= len;
    Some(ImageData {
        width: dimensions[0],
        height: dimensions[1],
        data: image.into_raw(),
    })
}

/// Straight sRGB content plus intrinsic/antialias coverage for the existing content compositor.
/// Brush alpha occurs once, in content; coverage excludes brush alpha (also for Overwrite).
pub struct TextRaster {
    pub content: Vec<u8>,
    pub coverage: Vec<u8>,
}
/// Rasterize ordered document glyphs. `space` is the consumer's effective compositing space.
/// Outline RGB uses the brush; bitmap RGB is intrinsic. The Text tool uses its existing sRGB
/// content compositor, while effect consumers can request linear-light glyph mixing.
pub fn rasterize_text_content(
    layout: &TextLayout,
    s: &TextSettings,
    rect: IntRect,
    brush: [u8; 4],
    space: Space,
) -> TextRaster {
    let n = rect.w as usize * rect.h as usize;
    if !layout
        .lines
        .iter()
        .any(|l| l.items.iter().any(|i| matches!(i, DrawItem::Bitmap(_))))
    {
        return TextRaster {
            content: brush.repeat(n),
            coverage: super::rasterize_text(layout, s, rect),
        };
    }
    let surface = opaque_surface(layout, s, rect, brush, space);
    let mut image = surface.to_rgba8(space, Alpha::Premultiplied);
    let coverage = image
        .data
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .map(|p| {
            let a = p[3];
            p[3] = brush[3];
            a
        })
        .collect();
    TextRaster {
        content: image.data,
        coverage,
    }
}

/// Premultiplied glyph pixels in the consumer's effective working space, before its
/// final selection/composition. Shared with TextRaster; brush alpha is applied once.
/// This avoids an intermediate sRGB quantization for GPU-style effect consumers.
pub fn rasterize_text_surface(
    layout: &TextLayout,
    s: &TextSettings,
    rect: IntRect,
    brush: [u8; 4],
    space: Space,
) -> EffectSurface {
    let mut surface = opaque_surface(layout, s, rect, brush, space);
    let alpha = brush[3] as f32 / 255.0;
    for pixel in &mut surface.px {
        for c in pixel {
            *c *= alpha;
        }
    }
    surface
}
fn opaque_surface(
    layout: &TextLayout,
    s: &TextSettings,
    rect: IntRect,
    brush: [u8; 4],
    space: Space,
) -> EffectSurface {
    let mut surface = EffectSurface::new(rect.w, rect.h);
    let ink = colour_exact(
        [brush[0], brush[1], brush[2], 255],
        RenderInfo::new(space, Alpha::Premultiplied),
    );
    for line in &layout.lines {
        for item in &line.items {
            let b = item.bounds();
            let left = b.x + line.origin[0];
            let top = b.y + line.origin[1];
            let area = IntRect::new(
                left.floor() as i32,
                top.floor() as i32,
                ((left + b.width).ceil() - left.floor()).max(0.0) as u32,
                ((top + b.height).ceil() - top.floor()).max(0.0) as u32,
            )
            .intersect(&rect);
            if area.is_empty() {
                continue;
            }
            match item {
                DrawItem::Outline(path) => {
                    let Some(mut pm) = tiny_skia::Pixmap::new(area.w, area.h) else {
                        continue;
                    };
                    let mut paint = tiny_skia::Paint::default();
                    paint.set_color_rgba8(255, 255, 255, 255);
                    paint.anti_alias = s.antialias;
                    pm.fill_path(
                        path,
                        &paint,
                        tiny_skia::FillRule::Winding,
                        Transform::from_translate(
                            (line.origin[0] - area.x as f64) as f32,
                            (line.origin[1] - area.y as f64) as f32,
                        ),
                        None,
                    );
                    for y in 0..area.h {
                        for x in 0..area.w {
                            let a = pm.data()[((y * area.w + x) * 4 + 3) as usize] as f32 / 255.0;
                            let i = ((area.y - rect.y + y as i32) as usize * rect.w as usize)
                                + (area.x - rect.x + x as i32) as usize;
                            surface.px[i] = source_over(surface.px[i], ink.map(|c| c * a));
                        }
                    }
                }
                DrawItem::Bitmap(bitmap) => {
                    let src = EffectSurface::from_rgba8(&bitmap.image, space, Alpha::Premultiplied);
                    for y in area.y..area.bottom() {
                        for x in area.x..area.right() {
                            let sx = ((x as f64 + 0.5 - left) / bitmap.scale) as f32;
                            let sy = ((y as f64 + 0.5 - top) / bitmap.scale) as f32;
                            let pixel = if s.antialias {
                                src.bilinear(sx, sy, Edge::Transparent)
                            } else {
                                src.point(sx, sy, Edge::Transparent)
                            };
                            let i = (y - rect.y) as usize * rect.w as usize + (x - rect.x) as usize;
                            surface.px[i] = source_over(surface.px[i], pixel);
                        }
                    }
                }
            }
        }
    }
    surface
}

// Bitmap cmap ranges of the approved NotoColorEmoji.ttf, SHA256 72a635cb3d2f3524
// c51620cdde406b217204e8a6a06c6a096ff8ed4b5fd6e27b. Metadata only: no font in initial WASM.
pub(super) fn bundled_bitmap_scalar(c: char) -> bool {
    matches!(c as u32,
        0x23..=0x23 |
        0x2a..=0x2a |
        0x30..=0x39 |
        0xa9..=0xa9 |
        0xae..=0xae |
        0x203c..=0x203c |
        0x2049..=0x2049 |
        0x20e3..=0x20e3 |
        0x2122..=0x2122 |
        0x2139..=0x2139 |
        0x2194..=0x2199 |
        0x21a9..=0x21aa |
        0x231a..=0x231b |
        0x2328..=0x2328 |
        0x23cf..=0x23cf |
        0x23e9..=0x23f3 |
        0x23f8..=0x23fa |
        0x24c2..=0x24c2 |
        0x25aa..=0x25ab |
        0x25b6..=0x25b6 |
        0x25c0..=0x25c0 |
        0x25fb..=0x25fe |
        0x2600..=0x2604 |
        0x260e..=0x260e |
        0x2611..=0x2611 |
        0x2614..=0x2615 |
        0x2618..=0x2618 |
        0x261d..=0x261d |
        0x2620..=0x2620 |
        0x2622..=0x2623 |
        0x2626..=0x2626 |
        0x262a..=0x262a |
        0x262e..=0x262f |
        0x2638..=0x263a |
        0x2640..=0x2640 |
        0x2642..=0x2642 |
        0x2648..=0x2653 |
        0x265f..=0x2660 |
        0x2663..=0x2663 |
        0x2665..=0x2666 |
        0x2668..=0x2668 |
        0x267b..=0x267b |
        0x267e..=0x267f |
        0x2692..=0x2697 |
        0x2699..=0x2699 |
        0x269b..=0x269c |
        0x26a0..=0x26a1 |
        0x26a7..=0x26a7 |
        0x26aa..=0x26ab |
        0x26b0..=0x26b1 |
        0x26bd..=0x26be |
        0x26c4..=0x26c5 |
        0x26c8..=0x26c8 |
        0x26ce..=0x26cf |
        0x26d1..=0x26d1 |
        0x26d3..=0x26d4 |
        0x26e9..=0x26ea |
        0x26f0..=0x26f5 |
        0x26f7..=0x26fa |
        0x26fd..=0x26fd |
        0x2702..=0x2702 |
        0x2705..=0x2705 |
        0x2708..=0x270d |
        0x270f..=0x270f |
        0x2712..=0x2712 |
        0x2714..=0x2714 |
        0x2716..=0x2716 |
        0x271d..=0x271d |
        0x2721..=0x2721 |
        0x2728..=0x2728 |
        0x2733..=0x2734 |
        0x2744..=0x2744 |
        0x2747..=0x2747 |
        0x274c..=0x274c |
        0x274e..=0x274e |
        0x2753..=0x2755 |
        0x2757..=0x2757 |
        0x2763..=0x2764 |
        0x2795..=0x2797 |
        0x27a1..=0x27a1 |
        0x27b0..=0x27b0 |
        0x27bf..=0x27bf |
        0x2934..=0x2935 |
        0x2b05..=0x2b07 |
        0x2b1b..=0x2b1c |
        0x2b50..=0x2b50 |
        0x2b55..=0x2b55 |
        0x3030..=0x3030 |
        0x303d..=0x303d |
        0x3297..=0x3297 |
        0x3299..=0x3299 |
        0x1f004..=0x1f004 |
        0x1f0cf..=0x1f0cf |
        0x1f170..=0x1f171 |
        0x1f17e..=0x1f17f |
        0x1f18e..=0x1f18e |
        0x1f191..=0x1f19a |
        0x1f1e6..=0x1f1ff |
        0x1f201..=0x1f202 |
        0x1f21a..=0x1f21a |
        0x1f22f..=0x1f22f |
        0x1f232..=0x1f23a |
        0x1f250..=0x1f251 |
        0x1f300..=0x1f321 |
        0x1f324..=0x1f393 |
        0x1f396..=0x1f397 |
        0x1f399..=0x1f39b |
        0x1f39e..=0x1f3f0 |
        0x1f3f3..=0x1f3f5 |
        0x1f3f7..=0x1f4fd |
        0x1f4ff..=0x1f53d |
        0x1f549..=0x1f54e |
        0x1f550..=0x1f567 |
        0x1f56f..=0x1f570 |
        0x1f573..=0x1f57a |
        0x1f587..=0x1f587 |
        0x1f58a..=0x1f58d |
        0x1f590..=0x1f590 |
        0x1f595..=0x1f596 |
        0x1f5a4..=0x1f5a5 |
        0x1f5a8..=0x1f5a8 |
        0x1f5b1..=0x1f5b2 |
        0x1f5bc..=0x1f5bc |
        0x1f5c2..=0x1f5c4 |
        0x1f5d1..=0x1f5d3 |
        0x1f5dc..=0x1f5de |
        0x1f5e1..=0x1f5e1 |
        0x1f5e3..=0x1f5e3 |
        0x1f5e8..=0x1f5e8 |
        0x1f5ef..=0x1f5ef |
        0x1f5f3..=0x1f5f3 |
        0x1f5fa..=0x1f64f |
        0x1f680..=0x1f6c5 |
        0x1f6cb..=0x1f6d2 |
        0x1f6d5..=0x1f6d8 |
        0x1f6dc..=0x1f6e5 |
        0x1f6e9..=0x1f6e9 |
        0x1f6eb..=0x1f6ec |
        0x1f6f0..=0x1f6f0 |
        0x1f6f3..=0x1f6fc |
        0x1f7e0..=0x1f7eb |
        0x1f7f0..=0x1f7f0 |
        0x1f90c..=0x1f93a |
        0x1f93c..=0x1f945 |
        0x1f947..=0x1f9ff |
        0x1fa70..=0x1fa7c |
        0x1fa80..=0x1fa8a |
        0x1fa8e..=0x1fac6 |
        0x1fac8..=0x1fac8 |
        0x1facd..=0x1fadc |
        0x1fadf..=0x1faea |
        0x1faef..=0x1faf8 |
        0xfe4e5..=0xfe4ee |
        0xfe82c..=0xfe82c |
        0xfe82e..=0xfe837
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    const NOTO: &[u8] =
        include_bytes!("../../../../../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf");
    fn settings() -> TextSettings {
        TextSettings {
            family: "Carlito".into(),
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
        }
    }
    fn layout(text: &str, s: &TextSettings, px: f64) -> TextLayout {
        let mut bytes = faces(s);
        bytes.truncate(3);
        bytes.push(Arc::from(NOTO));
        let mut l = line(&text.encode_utf16().collect::<Vec<_>>(), s, px, &bytes);
        l.origin = [10.0, 10.0];
        TextLayout {
            space_height: l.height,
            lines: vec![l],
        }
    }
    fn count(l: &TextLayout) -> usize {
        l.lines[0]
            .items
            .iter()
            .filter(|i| matches!(i, DrawItem::Bitmap(_)))
            .count()
    }
    #[test]
    fn all_calendar_defaults_have_real_palette_pixels_at_twelve_px() {
        // Exact 30 default text sequences; no Calendar feature is implemented here.
        let defaults = [
            "🎂Birthday",
            "🌴\u{200b}Vacation",
            "🥂New Year's Day",
            "❤\u{fe0f}Valentine's Day",
            "⏰Spring Forward",
            "⏰Fall Back",
            "🎩President's Day",
            "✝\u{fe0f}Good Friday",
            "🥚Easter",
            "🍀St. Patrick's",
            "🌱Spring Begins",
            "🌎Earth Day",
            "🪦Memorial Day",
            "⭐Flag Day",
            "☀\u{fe0f}Summer Begins",
            "Independence Day",
            "🔧Labor Day",
            "🍁Autum Begins",
            "🎃Halloween",
            "🎖Veteran's Day",
            "🍗Thanksgiving",
            "🏷Black Friday",
            "💻Cyber Monday",
            "🤲Giving Tuesday",
            "❄\u{fe0f}Winter Begins",
            "🎁Christmas Eve",
            "🎄Christmas",
            "🥂New Year's Eve",
            "🥂New Year's Day",
            "🥂New Year's Eve",
        ];
        for (i, text) in defaults.iter().enumerate() {
            let l = layout(text, &settings(), 12.0);
            assert_eq!(count(&l), usize::from(i != 15), "default {} {text}", i + 1);
            let r = rasterize_text_content(
                &l,
                &settings(),
                IntRect::new(0, 0, 220, 50),
                [0, 0, 0, 255],
                Space::Srgb,
            );
            let colors: std::collections::HashSet<_> = r
                .content
                .as_chunks::<4>()
                .0
                .iter()
                .zip(&r.coverage)
                .filter(|(p, a)| **a > 100 && (p[0] != p[1] || p[1] != p[2]))
                .map(|(p, _)| [p[0], p[1], p[2]])
                .collect();
            if i != 15 {
                assert!(
                    colors.len() >= 3,
                    "default {} must contain a palette, got {}",
                    i + 1,
                    colors.len()
                );
            }
        }
    }
    #[test]
    fn actual_strike_palette_metrics_and_explicit_units() {
        let f = ttf_parser::Face::parse(NOTO, 0).unwrap();
        let gid = f.glyph_index('🎂').unwrap();
        let g = f.glyph_raster_image(gid, 12).unwrap();
        assert_eq!(
            (g.pixels_per_em, g.width, g.height, g.x, g.y),
            (109, 136, 128, 0, -27)
        );
        let mut budget = MAX_RUN_BYTES;
        let image = decode_png(g.data, [136, 128], &mut budget).unwrap();
        assert_eq!(budget, MAX_RUN_BYTES - 136 * 128 * 4);
        assert!(image.data.as_chunks::<4>().0.iter().any(|p| p[3] == 0));
        assert!(
            image
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[3] == 255 && p[0] != p[1])
        );
        let l = layout("🎂", &settings(), 12.0);
        assert!((l.lines[0].width - 2550.0 * 12.0 / 2048.0).abs() < 0.00001);
        let DrawItem::Bitmap(b) = &l.lines[0].items[0] else {
            panic!("bitmap")
        };
        assert!((b.scale - 12.0 / 109.0).abs() < 1e-10);
        assert_eq!(l.lines[0].glyphs[0].utf16, 0..2);
        assert_eq!(l.lines[0].glyphs[0].glyph_id, gid.0);
        assert_eq!(&*l.lines[0].glyphs[0].face, NOTO);
        assert_eq!(l.lines[0].glyphs[0].advance, l.lines[0].width);
        assert!(l.lines[0].glyphs[0].ink.is_some());
        assert!(
            (b.origin[1]
                - (metrics(fonts::CARLITO_REGULAR, 12.0)
                    .0
                    .max(1900.0 * 12.0 / 2048.0)
                    - 101.0 * 12.0 / 109.0))
                .abs()
                < 1e-10
        );
        fonts::register_lazy_face("task35c-units", FaceStyle::Regular, NOTO.to_vec()).unwrap();
        let mut s = settings();
        s.family = "task35c-units".into();
        s.size = 16.0;
        let px = layout_text_px("🎂", &s, [0.0, 0.0], 12.0);
        let fixed = layout_text("🎂", &s, [0.0, 0.0], 300.0);
        assert!((px.lines[0].width - 2550.0 * 12.0 / 2048.0).abs() < 1e-5);
        // skrifa retains the existing 26.6 em-size quantization before scaling hmtx.
        assert_eq!(fixed.lines[0].width, 2550.0 * (1365.0 / 64.0) / 2048.0);
    }
    #[test]
    fn pinned_strike_scales_at_small_large_and_adjacent_sizes() {
        let face = ttf_parser::Face::parse(NOTO, 0).unwrap();
        let gid = face.glyph_index('🎂').unwrap();
        for px in [8.0, 72.0, 108.0, 109.0, 110.0] {
            let g = face.glyph_raster_image(gid, px as u16).unwrap();
            assert_eq!(g.pixels_per_em, 109);
            let l = layout("🎂", &settings(), px);
            let DrawItem::Bitmap(b) = &l.lines[0].items[0] else {
                panic!("size {px} lost bitmap")
            };
            assert!((b.scale - px / 109.0).abs() < 1e-10);
            assert!((l.lines[0].width - 2550.0 * px / 2048.0).abs() < 0.001);
            let primary_ascent = metrics(fonts::CARLITO_REGULAR, px).0;
            let expected_top = primary_ascent.max(1900.0 * px / 2048.0) - 101.0 * px / 109.0;
            assert!((b.origin[1] - expected_top).abs() < 0.001);
            let bounds = l.lines[0].items[0].bounds();
            assert!((bounds.width - 136.0 * px / 109.0).abs() < 0.001);
            assert!((bounds.height - 128.0 * px / 109.0).abs() < 0.001);
        }
    }
    #[test]
    fn fractional_bitmap_edges_and_moved_shrunk_preview_keep_pixels() {
        let mut s = settings();
        s.antialias = false;
        let mut l = layout("🎂", &s, 12.0);
        l.lines[0].origin = [0.0, 0.0];
        let image = Arc::new(ImageData {
            width: 2,
            height: 2,
            data: vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
            ],
        });
        let cases = [
            ([-0.25, -0.25], 1.0, IntRect::new(0, 0, 1, 1), [255, 0, 0]),
            ([0.25, -0.25], 1.0, IntRect::new(1, 0, 1, 1), [0, 255, 0]),
            ([-0.25, 0.25], 1.0, IntRect::new(0, 1, 1, 1), [0, 0, 255]),
            ([0.25, 0.25], 1.0, IntRect::new(1, 1, 1, 1), [255, 255, 0]),
            ([1.25, 1.25], 0.5, IntRect::new(1, 1, 1, 1), [255, 0, 0]),
        ];
        for (origin, scale, rect, expected) in cases {
            l.lines[0].items = vec![DrawItem::Bitmap(Bitmap {
                image: image.clone(),
                origin,
                scale,
            })];
            let r = rasterize_text_content(&l, &s, rect, [4, 5, 6, 255], Space::Srgb);
            assert_eq!(r.coverage, [255], "origin={origin:?} scale={scale}");
            assert_eq!(
                &r.content[..3],
                &expected,
                "origin={origin:?} scale={scale}"
            );
        }
    }
    #[test]
    fn selectors_zero_width_space_and_need_match_actual_fallback() {
        let s = settings();
        assert_eq!(
            count(&layout("☀", &s, 12.0)),
            0,
            "Carlito has a sun outline"
        );
        assert_eq!(count(&layout("☀\u{fe0f}", &s, 12.0)), 1);
        assert_eq!(count(&layout("☀\u{fe0e}", &s, 12.0)), 0);
        assert_eq!(count(&layout("🎂\u{fe0e}", &s, 12.0)), 0);
        assert_eq!(
            layout("🌴\u{200b}Vacation", &s, 12.0).lines[0].width,
            layout("🌴Vacation", &s, 12.0).lines[0].width
        );
        for text in [
            "",
            "ordinary Abé 123 #*",
            "☀",
            "☀\u{fe0e}",
            "🎂\u{fe0e}",
            "\u{200b}",
            "\u{fe0f}",
        ] {
            assert!(!needs_color_font(text, &s), "{text}");
        }
        for text in ["🎂", "☀\u{fe0f}", "🌴\u{200b}Vacation"] {
            assert!(needs_color_font(text, &s), "{text}");
        }
    }
    #[test]
    fn unsupported_emoji_sequences_are_scalar_fallbacks() {
        let s = settings();
        for text in ["👩\u{200d}💻", "👍🏽", "🇺🇸", "🏴\u{200d}☠\u{fe0f}"] {
            let l = layout(text, &s, 12.0);
            assert!(count(&l) >= 2, "{text} must stay separate scalars");
            assert!(l.lines[0].width.is_finite() && l.lines[0].width > 0.0);
            let r = rasterize_text_content(
                &l,
                &s,
                IntRect::new(0, 0, 100, 35),
                [0, 0, 0, 255],
                Space::Srgb,
            );
            assert!(r.coverage.iter().any(|&a| a > 0));
        }
    }
    #[test]
    fn brush_alpha_once_selection_overwrite_and_effective_space() {
        let s = settings();
        let l = layout("🎂", &s, 30.0);
        let rect = IntRect::new(0, 0, 80, 70);
        let opaque = rasterize_text_content(&l, &s, rect, [3, 17, 251, 255], Space::Srgb);
        let half = rasterize_text_content(&l, &s, rect, [3, 17, 251, 128], Space::Srgb);
        assert_eq!(opaque.coverage, half.coverage);
        for (a, b) in opaque
            .content
            .as_chunks::<4>()
            .0
            .iter()
            .zip(half.content.as_chunks::<4>().0.iter())
        {
            assert_eq!(&a[..3], &b[..3]);
            assert_eq!(b[3], 128);
        }
        for destination_alpha in [0, 73, 255] {
            for selection_alpha in [0, 128, 255] {
                let background = [17, 31, 47, destination_alpha].repeat((rect.w * rect.h) as usize);
                let selection = vec![selection_alpha; (rect.w * rect.h) as usize];
                for mode in [
                    ContentBlendMode::Overwrite,
                    ContentBlendMode::Layer(crate::document::layer::BlendMode::Normal),
                    ContentBlendMode::Layer(crate::document::layer::BlendMode::Multiply),
                ] {
                    let output = crate::pixel::content::render_content(
                        &background,
                        rect.w,
                        rect,
                        &[ContentLayer {
                            content: &half.content,
                            mask: Some(&half.coverage),
                        }],
                        Some(&selection),
                        mode,
                    );
                    for (i, p) in output.as_chunks::<4>().0.iter().enumerate() {
                        if half.coverage[i] == 0 || selection_alpha == 0 {
                            assert_eq!(p, &background[i * 4..i * 4 + 4]);
                        }
                    }
                    let i = half.coverage.iter().position(|a| *a == 255).unwrap();
                    let expected = crate::pixel::content::render_content(
                        &background[i * 4..i * 4 + 4],
                        1,
                        IntRect::new(0, 0, 1, 1),
                        &[ContentLayer {
                            content: &half.content[i * 4..i * 4 + 4],
                            mask: Some(&[255]),
                        }],
                        Some(&[selection_alpha]),
                        mode,
                    );
                    assert_eq!(&output[i * 4..i * 4 + 4], expected.as_slice());
                }
            }
        }
        let zero = rasterize_text_surface(&l, &s, rect, [3, 17, 251, 0], Space::Linear);
        assert!(zero.px.iter().all(|p| *p == [0.0; 4]));
        let full = rasterize_text_surface(&l, &s, rect, [3, 17, 251, 255], Space::Linear);
        let half_surface = rasterize_text_surface(&l, &s, rect, [3, 17, 251, 128], Space::Linear);
        for (a, b) in full.px.iter().zip(&half_surface.px) {
            for i in 0..4 {
                assert!((b[i] - a[i] * (128.0 / 255.0)).abs() < 1e-7);
            }
        }
        let linear = rasterize_text_content(&l, &s, rect, [3, 17, 251, 128], Space::Linear);
        assert_eq!(linear.coverage, half.coverage);
        assert_ne!(
            linear.content, half.content,
            "colored bilinear edges must mix in requested working space"
        );
        let src = EffectSurface::from_rgba8(
            &ImageData {
                width: 2,
                height: 1,
                data: vec![255, 0, 0, 255, 0, 255, 0, 0],
            },
            Space::Srgb,
            Alpha::Premultiplied,
        );
        assert_eq!(
            src.bilinear(1.0, 0.5, Edge::Transparent),
            [0.5, 0.0, 0.0, 0.5],
            "transparent RGB must not bleed into bitmap sampling"
        );
    }
    #[test]
    fn known_partial_alpha_glyph_matches_kernel_b_and_ordered_effect_surface() {
        // Independent integer oracle from compositing.md §2.7 for a 50% intrinsic
        // glyph over a partially transparent destination. These are literal
        // expected pixels, not a second invocation of render_content.
        let mut s = settings();
        s.antialias = false;
        let mut l = layout("🎂", &s, 12.0);
        l.lines[0].origin = [0.0, 0.0];
        let bitmap = |rgba: [u8; 4]| {
            DrawItem::Bitmap(Bitmap {
                image: Arc::new(ImageData {
                    width: 1,
                    height: 1,
                    data: rgba.to_vec(),
                }),
                origin: [0.0, 0.0],
                scale: 1.0,
            })
        };
        l.lines[0].items = vec![bitmap([200, 40, 20, 128])];
        let rect = IntRect::new(0, 0, 1, 1);
        let modes = [
            ContentBlendMode::Layer(crate::document::layer::BlendMode::Normal),
            ContentBlendMode::Overwrite,
            ContentBlendMode::Layer(crate::document::layer::BlendMode::Multiply),
        ];
        let expected: [[[u8; 4]; 3]; 9] = [
            [[17, 31, 47, 73], [17, 31, 47, 73], [17, 31, 47, 73]],
            [[17, 31, 47, 73], [17, 31, 47, 55], [17, 31, 47, 73]],
            [[17, 31, 47, 73], [17, 31, 47, 36], [17, 31, 47, 73]],
            [[17, 31, 47, 73], [17, 31, 47, 73], [17, 31, 47, 73]],
            [[78, 34, 38, 96], [84, 34, 37, 87], [60, 30, 36, 96]],
            [[115, 35, 32, 119], [134, 36, 29, 100], [87, 30, 30, 119]],
            [[17, 31, 47, 73], [17, 31, 47, 73], [17, 31, 47, 73]],
            [[115, 35, 32, 119], [115, 35, 32, 119], [87, 30, 30, 119]],
            [[159, 38, 25, 164], [159, 38, 25, 164], [117, 30, 22, 164]],
        ];
        for (bi, brush_alpha) in [0, 128, 255].into_iter().enumerate() {
            let raster =
                rasterize_text_content(&l, &s, rect, [9, 19, 29, brush_alpha], Space::Srgb);
            assert_eq!(raster.coverage, [128]);
            assert_eq!(raster.content, [200, 40, 20, brush_alpha]);
            for (si, selection_alpha) in [0, 128, 255].into_iter().enumerate() {
                for (mi, mode) in modes.into_iter().enumerate() {
                    let output = crate::pixel::content::render_content(
                        &[17, 31, 47, 73],
                        1,
                        rect,
                        &[ContentLayer {
                            content: &raster.content,
                            mask: Some(&raster.coverage),
                        }],
                        Some(&[selection_alpha]),
                        mode,
                    );
                    assert_eq!(
                        output,
                        expected[bi * 3 + si][mi],
                        "brush={brush_alpha} selection={selection_alpha} mode={mode:?}"
                    );
                }
            }
        }

        // Two partially transparent color glyphs at the same pixel must be
        // ordered source-over in the effect consumer's premultiplied space.
        l.lines[0].items = vec![bitmap([255, 0, 0, 128]), bitmap([0, 0, 255, 128])];
        let surface = rasterize_text_surface(&l, &s, rect, [1, 2, 3, 255], Space::Srgb);
        let a = 128.0 / 255.0;
        let expected_surface = [a * (1.0 - a), 0.0, a, a + a * (1.0 - a)];
        for (actual, expected) in surface.px[0].into_iter().zip(expected_surface) {
            assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
        }
    }
    #[test]
    fn repeated_glyphs_share_bounded_decode_without_losing_color() {
        let l = layout(&"🎂".repeat(300), &settings(), 12.0);
        assert_eq!(count(&l), 300);
        let DrawItem::Bitmap(first) = &l.lines[0].items[0] else {
            panic!("bitmap")
        };
        let DrawItem::Bitmap(last) = l.lines[0].items.last().unwrap() else {
            panic!("bitmap")
        };
        assert!(Arc::ptr_eq(&first.image, &last.image));
        assert!(last.origin[0] > first.origin[0]);
    }
    #[test]
    fn corrupt_color_glyph_retains_deterministic_outline_fallback() {
        let f = ttf_parser::Face::parse(NOTO, 0).unwrap();
        let g = f
            .glyph_raster_image(f.glyph_index('🎂').unwrap(), 12)
            .unwrap();
        let offset = g.data.as_ptr() as usize - NOTO.as_ptr() as usize;
        let mut corrupt = NOTO.to_vec();
        corrupt[offset] = 0;
        let s = settings();
        let mut bytes = faces(&s);
        bytes.truncate(3);
        bytes.push(Arc::from(corrupt));
        let l = line(&"🎂".encode_utf16().collect::<Vec<_>>(), &s, 12.0, &bytes);
        assert!(
            l.items.iter().any(|i| matches!(i, DrawItem::Outline(_))),
            "bad bitmap must leave a visible missing-glyph fallback"
        );
        assert!(l.width.is_finite() && l.width > 0.0);

        let mut malformed = NOTO.to_vec();
        malformed[10_595_608..10_595_612].copy_from_slice(&0xfffffff0u32.to_be_bytes());
        assert!(fonts::validate_font(&malformed).is_ok());
        let mut bytes = faces(&s);
        bytes.truncate(3);
        bytes.push(Arc::from(malformed));
        let l = line(&"🎂".encode_utf16().collect::<Vec<_>>(), &s, 12.0, &bytes);
        assert!(
            l.items.iter().any(|i| matches!(i, DrawItem::Outline(_))),
            "selected CBLC offset overflow must use the ordinary missing-glyph outline"
        );
        assert!(l.width.is_finite() && l.width > 0.0);
    }
    #[test]
    fn malformed_oversize_mismatched_and_budget_exhausted_pngs_are_rejected() {
        let f = ttf_parser::Face::parse(NOTO, 0).unwrap();
        let g = f
            .glyph_raster_image(f.glyph_index('🎂').unwrap(), 12)
            .unwrap();
        for data in [&b"bad"[..], &g.data[..g.data.len() / 2]] {
            assert!(decode_png(data, [136, 128], &mut { MAX_RUN_BYTES }).is_none());
        }
        assert!(decode_png(g.data, [137, 128], &mut { MAX_RUN_BYTES }).is_none());
        assert!(decode_png(g.data, [65535, 65535], &mut { MAX_RUN_BYTES }).is_none());
        assert!(decode_png(g.data, [136, 128], &mut 16).is_none());
        let mut budget = 136 * 128 * 4;
        assert!(decode_png(g.data, [136, 128], &mut budget).is_some());
        assert_eq!(budget, 0);
        assert!(decode_png(g.data, [136, 128], &mut budget).is_none());
    }
    #[test]
    fn colored_and_outline_items_keep_order_including_overlapping_decoration() {
        let mut s = settings();
        s.underline = true;
        s.strikeout = true;
        let mut l = layout("A🎂B", &s, 48.0);
        let items = &l.lines[0].items;
        let bitmap = items
            .iter()
            .position(|i| matches!(i, DrawItem::Bitmap(_)))
            .unwrap();
        assert!(bitmap > 0 && bitmap + 1 < items.len());
        assert!(matches!(items[0], DrawItem::Outline(_)));
        let rect = IntRect::new(0, 0, 240, 100);
        let first = rasterize_text_content(&l, &s, rect, [0, 0, 255, 255], Space::Srgb);
        // Place a brush outline over the color glyph and reverse that exact draw order.
        let b = l.lines[0].items[bitmap].bounds();
        let path = PathBuilder::from_rect(
            tiny_skia::Rect::from_xywh(b.x as f32, b.y as f32, b.width as f32, b.height as f32)
                .unwrap(),
        );
        l.lines[0].items.push(DrawItem::Outline(path));
        let last = rasterize_text_content(&l, &s, rect, [0, 0, 255, 255], Space::Srgb);
        l.lines[0].items.rotate_right(1);
        let behind = rasterize_text_content(&l, &s, rect, [0, 0, 255, 255], Space::Srgb);
        assert_ne!(first.content, last.content);
        assert_ne!(behind.content, last.content);
    }
}
