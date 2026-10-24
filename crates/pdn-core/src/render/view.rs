//! The canvas view: surround, drop shadow, checkerboard and the document, drawn into a device-pixel
//! `DisplayTarget` the way Paint.NET's Direct2D canvas layers draw them.
//! Port of: js/src/engine/document/render/renderer/SurfaceBoxBaseRenderer.js (its role only; the
//! pixels follow Paint.NET)
//! Reference: paintdotjs @ b4e6fc5

use super::checker::{checker_colors, checker_is_light, checker_square};
use super::display::{DisplayTarget, RenderMode, blend_over, for_bands, parts, store};
use crate::document::surface::Surface;
use crate::pixel::gamma::{SRGB8_TO_LINEAR, linear_to_srgb_f32, quantize};
use crate::pixel::{IntRect, Px};
use crate::resample::{Gamma, Kernel, magic, widen};

/// device = image·scale + origin (Paint.NET: device = (canvas − offset)·s, origin = −offset·s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewTransform {
    /// Device px per image px (zoom ratio; 1.0 = 100 %).
    pub scale: f64,
    /// Device position of the image's top-left corner, relative to the viewport origin.
    pub origin: [f64; 2],
    /// Viewport size in device px.
    pub viewport: [u32; 2],
    /// UI scale u = pixels per DIP.
    pub ui_scale: f64,
}

impl ViewTransform {
    /// §2.5 step 7: at integer scale or in fit mode the origin moves to ⌈origin⌉ (offset′ =
    /// offset + (⌊offset·s⌋ − offset·s)/s); otherwise unchanged.
    pub fn snapped(self, fit_mode: bool) -> Self {
        if self.scale.fract() != 0.0 && !fit_mode {
            return self;
        }
        Self {
            origin: self.origin.map(f64::ceil),
            ..self
        }
    }

    /// The device px an edit of document rect [l, r) × [t, b) can change, ∩ the viewport: its
    /// image, widened by `DIRTY_MARGIN` for what sampling reaches.
    pub fn dirty_rect(&self, [l, t, r, b]: [f64; 4]) -> IntRect {
        let edge = |v: f64| v.clamp(-1e9, 1e9) as i32;
        let (s, [ox, oy]) = (self.scale, self.origin);
        let m = DIRTY_MARGIN as f64;
        let (x0, y0) = (
            edge((ox + l * s).floor() - m),
            edge((oy + t * s).floor() - m),
        );
        let (x1, y1) = (edge((ox + r * s).ceil() + m), edge((oy + b * s).ceil() + m));
        let rect = IntRect::new(x0, y0, (x1 - x0).max(0) as u32, (y1 - y0).max(0) as u32);
        rect.intersect(&IntRect::new(0, 0, self.viewport[0], self.viewport[1]))
    }

    /// §2.5 step 8: [⌈L⌉, ⌈R⌉) × [⌈T⌉, ⌈B⌉) of the image rect in device px (f32 evaluation).
    pub fn image_pixel_bounds(&self, w: u32, h: u32) -> IntRect {
        // Clamped far inside i32 so the rect arithmetic below never overflows.
        let edge = |v: f64| ((v as f32).ceil() as f64).clamp(-1e9, 1e9) as i32;
        let (l, t) = (edge(self.origin[0]), edge(self.origin[1]));
        let r = edge(self.origin[0] + w as f64 * self.scale);
        let b = edge(self.origin[1] + h as f64 * self.scale);
        IntRect::new(l, t, (r - l).max(0) as u32, (b - t).max(0) as u32)
    }
}

/// Device px an edit can reach beyond its image: the bilinear neighbour, the LinearMultisample4x
/// offsets and, zoomed out, the mip texels a Linear-kernel edge tile spreads it to (§2.2 step 4(c),
/// §2.5 step 4).
pub const DIRTY_MARGIN: i32 = 3;

/// The look of the render layers below the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasStyle {
    pub surround: Px,
    /// Straight sRGB shadow colour and alpha; `None` when the drop shadow is off.
    pub shadow: Option<(Px, u8)>,
    pub checker_brightness: f32,
    pub mode: RenderMode,
    /// A pan or zoom in progress (`CanvasPixels::render`'s draft): LinearMultisample4x drops to
    /// one Linear tap. The settled view is drawn without it.
    pub draft: bool,
}

impl CanvasStyle {
    /// §2.7 step 1 and spec §0.10: dark → surround #303030, shadow white α 192; light → #CFCFCF,
    /// black α 255. `custom_surround` (`canvas.customBorder` + `canvas.borderColor`) replaces the
    /// surround colour (`canvas-rendering.md §5 "Task 51"`); the surround is always opaque.
    pub fn for_scheme(
        dark: bool,
        drop_shadow: bool,
        custom_surround: Option<Px>,
        checker_brightness: f32,
        mode: RenderMode,
    ) -> Self {
        let theme = if dark { [0x30; 3] } else { [0xCF; 3] };
        let [r, g, b] = custom_surround.map_or(theme, |c| [c[0], c[1], c[2]]);
        let shadow = if dark {
            ([255, 255, 255, 255], 192)
        } else {
            ([0, 0, 0, 255], 255)
        };
        Self {
            surround: [r, g, b, 255],
            shadow: drop_shadow.then_some(shadow),
            checker_brightness,
            mode,
            draft: false,
        }
    }
}

/// One stored mip level N: premultiplied linear u16 (Hardware) or premultiplied sRGB u8
/// (Software), with a dirty flag per 2^(7+N) source tile.
struct Mip {
    n: u32,
    w: u32,
    h: u32,
    data: MipData,
    /// Row-major over the level's tiles (128 mip px square).
    dirty: Vec<bool>,
}

#[derive(Debug, PartialEq)]
enum MipData {
    Linear(Vec<[u16; 4]>),
    Srgb(Vec<[u8; 4]>),
}

impl Mip {
    fn tiles_x(&self) -> u32 {
        self.w.div_ceil(128)
    }
}

/// Mip pyramid of the composite (§2.2 step 2). A level is built only when a frame needs it,
/// directly from the composite, per 2^(7+N) source tile in linear premultiplied f32 — MagicScaler
/// `Kernel::Average` when the tile divides evenly, else `Kernel::Linear` (§2.2 step 4(c)) — and
/// stored as premultiplied linear u16 (Hardware; Paint.NET uses f16) or premultiplied sRGB u8
/// (Software). Level 0 is sampled from the composite directly.
///
/// `invalidate` marks the tiles an edit touched at every level (§2.3); they are rebuilt when the
/// level is next drawn, so the chain stays equal to a fresh build. A revision `invalidate` did not
/// announce, a new mode or a new size drops every level.
#[derive(Default)]
pub struct MipChain {
    key: Option<(RenderMode, u32, u32)>,
    /// The composite revision the levels (with their dirty tiles rebuilt) show.
    revision: Option<u64>,
    levels: Vec<Option<Mip>>,
    builds: usize,
}

impl MipChain {
    /// Times a whole level was built (test hook; Review Focus 3).
    pub fn builds(&self) -> usize {
        self.builds
    }

    /// The composite changed inside `rect` (document px; `None` = everywhere) and its revision
    /// advanced by one.
    pub fn invalidate(&mut self, rect: Option<IntRect>) {
        let Some(revision) = &mut self.revision else {
            return;
        };
        *revision += 1;
        let Some(rect) = rect else {
            self.levels.clear();
            return;
        };
        if rect.is_empty() {
            return;
        }
        for mip in self.levels.iter_mut().flatten() {
            let side = 1i64 << (7 + mip.n);
            let tiles_x = mip.tiles_x() as i64;
            let tiles_y = mip.h.div_ceil(128) as i64;
            let span = |lo: i32, hi: i32, n: i64| {
                let first = (lo as i64).div_euclid(side).clamp(0, n);
                first..((hi as i64 - 1).div_euclid(side) + 1).clamp(first, n)
            };
            for ty in span(rect.y, rect.bottom(), tiles_y) {
                for tx in span(rect.x, rect.right(), tiles_x) {
                    mip.dirty[(ty * tiles_x + tx) as usize] = true;
                }
            }
        }
    }

    fn level(&mut self, composite: &Surface, revision: u64, mode: RenderMode, n: u32) -> &Mip {
        let key = (mode, composite.width(), composite.height());
        if self.key != Some(key) || self.revision != Some(revision) {
            self.levels.clear();
            self.key = Some(key);
            self.revision = Some(revision);
        }
        let n = n as usize;
        if self.levels.len() <= n {
            self.levels.resize_with(n + 1, || None);
        }
        let mip = self.levels[n].get_or_insert_with(|| {
            self.builds += 1;
            new_mip(composite.width(), composite.height(), n as u32, mode)
        });
        if mip.dirty.contains(&true) {
            build_tiles(composite, mip);
        }
        mip
    }
}

/// An all-dirty level N for a w×h composite.
fn new_mip(w: u32, h: u32, n: u32, mode: RenderMode) -> Mip {
    let (mw, mh) = (w.div_ceil(1 << n), h.div_ceil(1 << n));
    let size = mw as usize * mh as usize;
    Mip {
        n,
        w: mw,
        h: mh,
        data: match mode {
            RenderMode::Hardware => MipData::Linear(vec![[0; 4]; size]),
            RenderMode::Software => MipData::Srgb(vec![[0; 4]; size]),
        },
        dirty: vec![true; (mw.div_ceil(128) * mh.div_ceil(128)) as usize],
    }
}

/// Rebuilds the dirty tiles of `mip`, a band of tile rows at a time (in parallel on native).
fn build_tiles(composite: &Surface, mip: &mut Mip) {
    let tiles_x = mip.tiles_x() as usize;
    let (n, mw, dirty) = (mip.n, mip.w as usize, &mip.dirty);
    let rows = 0..mip.h as usize;
    match &mut mip.data {
        MipData::Linear(d) => for_bands(d, mw, rows, 128, |y0, band| {
            let row = &dirty[y0 / 128 * tiles_x..][..tiles_x];
            build_tile_row(composite, n, y0 / 128, row, band, |p| {
                p.map(|c| (c.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16)
            })
        }),
        MipData::Srgb(d) => for_bands(d, mw, rows, 128, |y0, band| {
            let row = &dirty[y0 / 128 * tiles_x..][..tiles_x];
            build_tile_row(composite, n, y0 / 128, row, band, premul_srgb8)
        }),
    }
    mip.dirty.fill(false);
}

/// §2.2 step 4(c) for the dirty tiles of tile row `ty` of level `n` ≥ 1: the clipped source tile
/// w×h, widened row by row to linear premultiplied f32 (`resample::widen`, `Gamma::Linear`),
/// resampled to ⌈w/2^n⌉ × ⌈h/2^n⌉ and stored through `conv` at (source offset >> n) of `band` (the
/// level's rows of this tile row). The resampler streams rows, so only a tile-sized f32 buffer is
/// held per tile.
fn build_tile_row<T>(
    composite: &Surface,
    n: u32,
    ty: usize,
    dirty: &[bool],
    band: &mut [T],
    conv: impl Fn([f32; 4]) -> T,
) {
    let (w, h) = (composite.width() as usize, composite.height() as usize);
    let mw = (w as u32).div_ceil(1 << n) as usize;
    let src = composite.data();
    let tile = 1usize << (7 + n);
    let sy = ty * tile;
    for (tx, _) in dirty.iter().enumerate().filter(|(_, d)| **d) {
        let sx = tx * tile;
        let (sw, sh) = (tile.min(w - sx), tile.min(h - sy));
        let (ow, oh) = (sw.div_ceil(1 << n), sh.div_ceil(1 << n));
        let kernel = if sw % ow == 0 && sh % oh == 0 {
            Kernel::Average
        } else {
            Kernel::Linear
        };
        magic::scale_rows(
            (sw, sh),
            (ow, oh),
            kernel,
            |y, line| {
                let at = ((sy + y) * w + sx) * 4;
                let row = src[at..at + sw * 4].as_chunks::<4>().0;
                for (p, &b) in line.iter_mut().zip(row) {
                    *p = widen(b, Gamma::Linear);
                }
            },
            |y, line| {
                let out = &mut band[y * mw + (sx >> n)..][..ow];
                for (o, &p) in out.iter_mut().zip(line) {
                    *o = conv(p);
                }
            },
        );
    }
}

/// Linear premultiplied → premultiplied sRGB bytes (the Software tile format).
// calibrate: B-C1 — Paint.NET's colour transform rounding is not documented.
fn premul_srgb8(p: [f32; 4]) -> [u8; 4] {
    let a = p[3];
    if a.is_nan() || a <= 0.0 {
        return [0; 4];
    }
    let c = |v: f32| quantize(linear_to_srgb_f32((v / a).min(1.0)) * a);
    [c(p[0]), c(p[1]), c(p[2]), quantize(a)]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interp {
    Nearest,
    Linear,
    LinearMultisample4x,
}

/// max(1, ⌊log₂ max(W,H)⌋ − 6).
pub fn mip_count(w: u32, h: u32) -> u32 {
    (w.max(h).max(1).ilog2() as i32 - 6).max(1) as u32
}

/// clamp(trunc(−log₂ s), 0, n − 1).
pub fn mip_level(scale: f64, n: u32) -> u32 {
    (-scale.log2())
        .trunc()
        .clamp(0.0, n.saturating_sub(1) as f64) as u32
}

/// §2.5 step 3, smooth mode: Software Linear; Hardware Linear when k < 1.01 and 1/k is within
/// 1/256 of a power of two, else LinearMultisample4x.
fn smooth(k: f64, mode: RenderMode) -> Interp {
    let inv = 1.0 / k;
    let pow2 = inv.log2().round().exp2();
    if mode == RenderMode::Software || (k < 1.01 && (inv - pow2).abs() < 1.0 / 256.0) {
        Interp::Linear
    } else {
        Interp::LinearMultisample4x
    }
}

/// §2.5 step 3: (mip, Some(c) for the NN pre-upscale by c = ⌈s⌉ at fractional s > 1, interpolation).
/// k = s·W/W_N, the scale from mip-N pixels to device px (x axis). Appendix B B-C4 calibrates it.
pub fn choose_interp(scale: f64, w: u32, h: u32, mode: RenderMode) -> (u32, Option<u32>, Interp) {
    let n = mip_level(scale, mip_count(w, h));
    if n == 0 && scale >= 1.0 {
        if scale.fract() == 0.0 {
            return (0, None, Interp::Nearest);
        }
        let c = scale.ceil();
        return (0, Some(c as u32), smooth(scale / c, mode));
    }
    let wn = w.div_ceil(1 << n).max(1);
    (n, None, smooth(scale * w as f64 / wn as f64, mode))
}

/// The LinearMultisample4x tap offsets in device px (§2.5 step 4.5): d₀ = (0.125, 0.375) and its
/// three 90° rotations.
const MULTISAMPLE: [[f64; 2]; 4] = [
    [0.125, 0.375],
    [-0.375, 0.125],
    [-0.125, -0.375],
    [0.375, -0.125],
];

/// §2.5 step 4.5: d₀ = (0.125, 0.375)/k and its three 90° rotations.
pub fn multisample_offsets(k: f64) -> [[f64; 2]; 4] {
    MULTISAMPLE.map(|[x, y]| [x / k, y / k])
}

/// §2.7 step 3: w = exp(−d²/(2σ²)) with σ = 1.5u.
pub fn shadow_weight(d: f64, u: f64) -> f64 {
    let sigma = 1.5 * u;
    (-d * d / (2.0 * sigma * sigma)).exp()
}

/// Draws Background (surround + shadow), Checkerboard and Document (§2.1 order) into `target`.
/// The document samples with the §2.5 rules: scene p = (x + ½, y + ½); q = (p − origin)/s in
/// image px, mapped into mip N by W_N/W; q clamped to [½, W_N − ½]; NN takes the texel containing
/// q, Linear is bilinear between texel centres, LinearMultisample4x = ½·B(q) + ⅛·Σ B(q + dᵢ);
/// the fractional-zoom path samples the NN-upscaled (×c) image without materializing it. Only
/// `target.clip()` is drawn, each pixel exactly as a full render draws it (rows run in parallel
/// bands on native targets).
pub fn render_view(
    composite: &Surface,
    revision: u64,
    mips: &mut MipChain,
    t: &ViewTransform,
    style: &CanvasStyle,
    target: &mut DisplayTarget,
) {
    render_view_in(composite, revision, mips, t, style, target, None);
}

/// The view of a `w × h` image with nothing in it: surround, shadow and checkerboard.
pub fn render_empty_in(
    w: u32,
    h: u32,
    t: &ViewTransform,
    style: &CanvasStyle,
    target: &mut DisplayTarget,
) {
    let bounds = t.image_pixel_bounds(w, h);
    let bg = Background::new(target, bounds, style, t.ui_scale);
    let checker = Checker::new(target, bounds, style.checker_brightness, t.ui_scale);
    paint_view::<Mip>(target, None, &bg, &checker, None);
}

/// `render_view` of just `spans` (one-row rects sorted by row, none overlapping) ∩ the clip, or of
/// the whole clip (`None`). One pass of row bands: each band draws Background, Checkerboard and
/// Document into its part of the clip, or into each of its spans.
pub fn render_view_in(
    composite: &Surface,
    revision: u64,
    mips: &mut MipChain,
    t: &ViewTransform,
    style: &CanvasStyle,
    target: &mut DisplayTarget,
    spans: Option<&[IntRect]>,
) {
    render_view_with(composite, revision, mips, t, style, target, spans, false);
}

/// `render_view_in` with the checkerboard's two colours swapped: what a pan draft shows where it
/// moved a square of the other colour under the pixels.
pub fn render_view_inverted_in(
    composite: &Surface,
    revision: u64,
    mips: &mut MipChain,
    t: &ViewTransform,
    style: &CanvasStyle,
    target: &mut DisplayTarget,
) {
    render_view_with(composite, revision, mips, t, style, target, None, true);
}

/// `render_view_in`, or with `inverted` the checkerboard's colours swapped.
#[allow(clippy::too_many_arguments)]
fn render_view_with(
    composite: &Surface,
    revision: u64,
    mips: &mut MipChain,
    t: &ViewTransform,
    style: &CanvasStyle,
    target: &mut DisplayTarget,
    spans: Option<&[IntRect]>,
    inverted: bool,
) {
    debug_assert_eq!(style.mode, target.mode);
    let (w, h) = (composite.width(), composite.height());
    let bounds = t.image_pixel_bounds(w, h);
    let bg = Background::new(target, bounds, style, t.ui_scale);
    let mut checker = Checker::new(target, bounds, style.checker_brightness, t.ui_scale);
    if inverted {
        (checker.light, checker.dark) = (checker.dark, checker.light);
    }
    // §2.5: the mip and interpolation, then every device pixel of the image pixel bounds (∩ the
    // clip) sampled and composited source-over onto the checkerboard (step 6).
    let area = clip(target, bounds.x, bounds.y, bounds.right(), bounds.bottom());
    if area.is_empty() || w == 0 || h == 0 {
        return paint_view::<Mip>(target, spans, &bg, &checker, None);
    }
    let (n, up, mut interp) = choose_interp(t.scale, w, h, style.mode);
    if style.draft && interp == Interp::LinearMultisample4x {
        interp = Interp::Linear;
    }
    let (t, at) = (*t, (w, h, up, interp, area));
    match (n, style.mode) {
        (0, RenderMode::Hardware) => {
            let src = CompositeLinear(composite);
            paint_view(
                target,
                spans,
                &bg,
                &checker,
                Some(&Document::new(&src, &t, at)),
            );
        }
        (0, RenderMode::Software) => {
            let src = CompositeSrgb(composite);
            paint_view(
                target,
                spans,
                &bg,
                &checker,
                Some(&Document::new(&src, &t, at)),
            );
        }
        _ => {
            let src = mips.level(composite, revision, style.mode, n);
            paint_view(
                target,
                spans,
                &bg,
                &checker,
                Some(&Document::new(src, &t, at)),
            );
        }
    }
}

/// The band pass of `render_view_in`.
fn paint_view<T: Texels + Sync>(
    target: &mut DisplayTarget,
    spans: Option<&[IntRect]>,
    bg: &Background,
    checker: &Checker,
    doc: Option<&Document<T>>,
) {
    let (clip, tw, mode) = (target.clip(), target.width as usize, target.mode);
    let rows = match spans {
        Some([first, .., last]) => first.y.max(clip.y)..last.bottom().min(clip.bottom()),
        Some([one]) => one.y.max(clip.y)..one.bottom().min(clip.bottom()),
        Some([]) => return,
        None => clip.y..clip.bottom(),
    };
    target.bands(rows, |y0, band| {
        let y1 = y0 + (band.len() / tw) as i32;
        let mut scratch = Scratch::default();
        for part in parts(clip, spans, y0, y1) {
            bg.paint(part, y0, tw, band);
            checker.paint(part, y0, tw, band);
            if let Some(d) = doc {
                d.paint(mode, part, y0, tw, band, &mut scratch);
            }
        }
    });
}

/// [l, r) × [t, b) clamped to the target's clip (empty when inverted).
fn clip(target: &DisplayTarget, l: i32, t: i32, r: i32, b: i32) -> IntRect {
    let c = target.clip();
    let (l, r) = (l.clamp(c.x, c.right()), r.clamp(c.x, c.right()));
    let (t, b) = (t.clamp(c.y, c.bottom()), b.clamp(c.y, c.bottom()));
    IntRect::new(l, t, (r - l).max(0) as u32, (b - t).max(0) as u32)
}

/// Row `y` of a full-width `band` of `tw`-wide rows starting at row `y0`.
fn band_row(band: &mut [[f32; 4]], y0: i32, tw: usize, y: i32) -> &mut [[f32; 4]] {
    &mut band[(y - y0) as usize * tw..][..tw]
}

/// §2.7 steps 2–4: the four surround rects, then the shadow bands (copy-compositing), each ∩ the
/// clip; no outline.
struct Background {
    mode: RenderMode,
    surround: [f32; 4],
    fills: [IntRect; 4],
    shadow: Option<Shadow>,
}

struct Shadow {
    /// The shadow and surround colours in target space.
    s: [f32; 4],
    bg: [f32; 4],
    u: f64,
    /// The image bounds inset by 1.5u: left, top, right, bottom.
    inner: [f64; 4],
    bands: [IntRect; 4],
}

impl Background {
    fn new(target: &DisplayTarget, b: IntRect, style: &CanvasStyle, u: f64) -> Self {
        let (vw, vh) = (target.width as i32, target.height as i32);
        let (l, t, r, bt) = (b.x, b.y, b.right(), b.bottom());
        let shadow = style.shadow.map(|(color, alpha)| {
            let c = (4.5 * u).ceil() as i32;
            let inset = 1.5 * u;
            Shadow {
                s: target.to_space([color[0], color[1], color[2], alpha]),
                bg: target.to_space(style.surround),
                u,
                inner: [
                    l as f64 + inset,
                    t as f64 + inset,
                    r as f64 - inset,
                    bt as f64 - inset,
                ],
                bands: [
                    clip(target, l - c, t, l, bt),
                    clip(target, l - c, t - c, r + c, t),
                    clip(target, r, t, r + c, bt),
                    clip(target, l - c, bt, r + c, bt + c),
                ],
            }
        });
        Background {
            mode: target.mode,
            surround: store(target.mode, target.to_space(style.surround)),
            fills: [
                clip(target, 0, t, l, bt),
                clip(target, 0, 0, vw, t),
                clip(target, r, t, vw, bt),
                clip(target, 0, bt, vw, vh),
            ],
            shadow,
        }
    }

    /// Its pixels in `part` of a band.
    fn paint(&self, part: IntRect, y0: i32, tw: usize, band: &mut [[f32; 4]]) {
        for r in self.fills.map(|r| r.intersect(&part)) {
            for y in r.y..r.bottom() {
                band_row(band, y0, tw, y)[r.x as usize..r.right() as usize].fill(self.surround);
            }
        }
        let Some(Shadow {
            s,
            bg,
            u,
            inner: [il, it, ir, ib],
            bands,
        }) = &self.shadow
        else {
            return;
        };
        for r in bands.map(|r| r.intersect(&part)) {
            for y in r.y..r.bottom() {
                let py = y as f64 + 0.5;
                let dy = (it - py).max(py - ib).max(0.0);
                let row = band_row(band, y0, tw, y);
                for x in r.x..r.right() {
                    let px = x as f64 + 0.5;
                    let dx = (il - px).max(px - ir).max(0.0);
                    let w = shadow_weight((dx * dx + dy * dy).sqrt(), *u) as f32;
                    let keep = 1.0 - w * s[3];
                    let out = std::array::from_fn(|i| {
                        if i == 3 {
                            1.0
                        } else {
                            w * s[i] * (w * s[3]) + bg[i] * keep
                        }
                    });
                    row[x as usize] = store(self.mode, out);
                }
            }
        }
    }
}

/// §2.6: Medium squares (8·SF px) over the canvas pixel bounds (∩ the clip), anchored at the
/// viewport origin.
struct Checker {
    area: IntRect,
    side: i32,
    light: [f32; 4],
    dark: [f32; 4],
}

impl Checker {
    fn new(target: &DisplayTarget, b: IntRect, brightness: f32, u: f64) -> Self {
        let (light, dark) = checker_colors(brightness);
        let [light, dark] =
            [light, dark].map(|g| store(target.mode, target.to_space([g, g, g, 255])));
        Checker {
            area: clip(target, b.x, b.y, b.right(), b.bottom()),
            side: checker_square(u, true) as i32,
            light,
            dark,
        }
    }

    fn paint(&self, part: IntRect, y0: i32, tw: usize, band: &mut [[f32; 4]]) {
        let (a, side) = (self.area.intersect(&part), self.side);
        for y in a.y..a.bottom() {
            let row = band_row(band, y0, tw, y);
            let mut x = a.x;
            while x < a.right() {
                // The run to the next square edge.
                let end = ((x.div_euclid(side) + 1) * side).min(a.right());
                let v = if checker_is_light(x, y, side as u32) {
                    self.light
                } else {
                    self.dark
                };
                row[x as usize..end as usize].fill(v);
                x = end;
            }
        }
    }
}

/// A level's texels in target space (premultiplied; linear in Hardware).
trait Texels {
    fn size(&self) -> (u32, u32);
    fn get(&self, x: u32, y: u32) -> [f32; 4];
}

/// Level 0 in Hardware (§2.2 step 4(b)): sRGB → linear, premultiplied in f32.
struct CompositeLinear<'a>(&'a Surface);
/// Level 0 in Software (§2.2 step 4(a)): Pbgra32, c′ = round(c·a/255) (compositing.md §2.6).
struct CompositeSrgb<'a>(&'a Surface);

fn texel(s: &Surface, x: u32, y: u32) -> [u8; 4] {
    let at = (y as usize * s.width() as usize + x as usize) * 4;
    let d = s.data();
    [d[at], d[at + 1], d[at + 2], d[at + 3]]
}

impl Texels for CompositeLinear<'_> {
    fn size(&self) -> (u32, u32) {
        (self.0.width(), self.0.height())
    }
    fn get(&self, x: u32, y: u32) -> [f32; 4] {
        let p = texel(self.0, x, y);
        let a = p[3] as f32 / 255.0;
        let c = |v: u8| SRGB8_TO_LINEAR[v as usize] * a;
        [c(p[0]), c(p[1]), c(p[2]), a]
    }
}

impl Texels for CompositeSrgb<'_> {
    fn size(&self) -> (u32, u32) {
        (self.0.width(), self.0.height())
    }
    fn get(&self, x: u32, y: u32) -> [f32; 4] {
        let p = texel(self.0, x, y);
        let a = p[3] as u32;
        // compositing.md §2.6 step 2: c′ = round(c·a/255) (c·a/255 is never a tie).
        let c = |v: u8| ((v as u32 * a + 127) / 255) as f32 / 255.0;
        [c(p[0]), c(p[1]), c(p[2]), a as f32 / 255.0]
    }
}

impl Texels for Mip {
    fn size(&self) -> (u32, u32) {
        (self.w, self.h)
    }
    fn get(&self, x: u32, y: u32) -> [f32; 4] {
        let at = y as usize * self.w as usize + x as usize;
        match &self.data {
            MipData::Linear(d) => d[at].map(|v| v as f32 / 65535.0),
            MipData::Srgb(d) => d[at].map(|v| v as f32 / 255.0),
        }
    }
}

/// One bilinear axis tap: texels `i0`, `i1` (source indices) and the weight of `i1`. NN is a tap
/// with `i1 = i0`.
#[derive(Clone, Copy)]
struct Tap {
    i0: u32,
    i1: u32,
    f: f32,
}

/// The sampling geometry of one axis: virtual size n·c (the ×c NN upscale), scale k from virtual
/// texels to device px, image origin in device px.
struct Axis {
    n: u32,
    c: u32,
    k: f64,
    origin: f64,
}

impl Axis {
    /// q = (p − origin)/k for device pixel `d`, clamped to [½, n·c − ½] (§2.5 step 4.3).
    fn q(&self, d: i32) -> f64 {
        let hi = (self.n as f64 * self.c as f64 - 0.5).max(0.5);
        ((d as f64 + 0.5 - self.origin) / self.k).clamp(0.5, hi)
    }

    /// NN: the texel containing q, as a source index.
    fn nearest(&self, q: f64) -> Tap {
        let i = (q as u32 / self.c).min(self.n - 1);
        Tap {
            i0: i,
            i1: i,
            f: 0.0,
        }
    }

    /// Bilinear between virtual texel centres i + ½, the position clamped first (`B` clamps).
    fn tap(&self, q: f64) -> Tap {
        let vn = self.n as f64 * self.c as f64;
        let u = q.clamp(0.5, (vn - 0.5).max(0.5)) - 0.5;
        let i0 = u.floor();
        let i1 = (i0 + 1.0).min(vn - 1.0);
        Tap {
            i0: (i0 as u32 / self.c).min(self.n - 1),
            i1: (i1 as u32 / self.c).min(self.n - 1),
            f: (u - i0) as f32,
        }
    }

    /// The taps one device pixel samples: q for NN and Linear; q, q + d₀ … q + d₃ for
    /// LinearMultisample4x (dᵢ = `MULTISAMPLE[i]`/k on this axis); unused entries repeat q.
    fn taps(&self, d: i32, interp: Interp, axis: usize) -> [Tap; 5] {
        let q = self.q(d);
        match interp {
            Interp::Nearest => [self.nearest(q); 5],
            Interp::Linear => [self.tap(q); 5],
            Interp::LinearMultisample4x => std::array::from_fn(|i| match i {
                0 => self.tap(q),
                _ => self.tap(q + MULTISAMPLE[i - 1][axis] / self.k),
            }),
        }
    }
}

/// Source rows converted to target space over one column span, holding just the rows the current
/// output row samples: taps only move down as the output row does.
#[derive(Default)]
struct RowCache {
    x0: u32,
    width: u32,
    first: u32,
    rows: std::collections::VecDeque<Vec<[f32; 4]>>,
    spare: Vec<Vec<[f32; 4]>>,
}

impl RowCache {
    /// Empty, over source columns `x0..x0 + width` (the rows' buffers kept for reuse).
    fn reset(&mut self, x0: u32, width: u32) {
        self.spare.extend(self.rows.drain(..));
        (self.x0, self.width, self.first) = (x0, width, 0);
    }

    /// Keeps rows `lo..=hi`, converting the missing ones.
    fn fill<T: Texels>(&mut self, src: &T, lo: u32, hi: u32) {
        while self.first < lo
            && let Some(row) = self.rows.pop_front()
        {
            self.spare.push(row);
            self.first += 1;
        }
        if self.rows.is_empty() {
            self.first = lo;
        }
        while self.first + (self.rows.len() as u32) <= hi {
            let y = self.first + self.rows.len() as u32;
            let mut row = self.spare.pop().unwrap_or_default();
            row.clear();
            row.extend((self.x0..self.x0 + self.width).map(|x| src.get(x, y)));
            self.rows.push_back(row);
        }
    }

    fn row(&self, y: u32) -> &[[f32; 4]] {
        &self.rows[(y - self.first) as usize]
    }

    /// B: bilinear between the cached texels (taps relative to `x0`).
    fn bilinear(&self, x: Tap, y: Tap) -> [f32; 4] {
        let (r0, r1) = (self.row(y.i0), self.row(y.i1));
        let (p00, p01) = (r0[x.i0 as usize], r0[x.i1 as usize]);
        let (p10, p11) = (r1[x.i0 as usize], r1[x.i1 as usize]);
        std::array::from_fn(|c| {
            let top = p00[c] + (p01[c] - p00[c]) * x.f;
            let bottom = p10[c] + (p11[c] - p10[c]) * x.f;
            top + (bottom - top) * y.f
        })
    }
}

/// The taps' source index range, (min i0, max i1).
fn tap_span(taps: &[Tap; 5]) -> (u32, u32) {
    let lo = taps.iter().map(|t| t.i0).min().unwrap_or(0);
    (lo, taps.iter().map(|t| t.i1).max().unwrap_or(0))
}

/// A band's buffers: the row cache and the re-based x taps of a part narrower than the area.
#[derive(Default)]
struct Scratch {
    cache: RowCache,
    taps: Vec<[Tap; 5]>,
}

/// §2.5 for one render: the level sampled, its y axis, and the x taps of every column of the
/// document `area` (the image pixel bounds ∩ the clip), relative to `x0`, the least column they
/// read (`x1` the greatest).
struct Document<'a, T> {
    src: &'a T,
    ay: Axis,
    interp: Interp,
    area: IntRect,
    xs: Vec<[Tap; 5]>,
    x0: u32,
    x1: u32,
}

impl<'a, T: Texels> Document<'a, T> {
    /// The sampling of `src` (level 0 or mip N of a w×h image, `up` its NN pre-upscale) by
    /// `interp` over `area`.
    fn new(
        src: &'a T,
        t: &ViewTransform,
        (w, h, up, interp, area): (u32, u32, Option<u32>, Interp, IntRect),
    ) -> Self {
        let (lw, lh) = src.size();
        let c = up.unwrap_or(1);
        // k = s·W/(W_N·c): s at level 0, s/c on the upscaled path, s·W/W_N for mip N.
        let ax = Axis {
            n: lw,
            c,
            k: t.scale * w as f64 / (lw as f64 * c as f64),
            origin: t.origin[0],
        };
        let ay = Axis {
            n: lh,
            c,
            k: t.scale * h as f64 / (lh as f64 * c as f64),
            origin: t.origin[1],
        };
        let mut xs: Vec<[Tap; 5]> = (area.x..area.right())
            .map(|x| ax.taps(x, interp, 0))
            .collect();
        let x0 = xs.iter().map(|t| tap_span(t).0).min().unwrap_or(0);
        let x1 = xs.iter().map(|t| tap_span(t).1).max().unwrap_or(0);
        for t in xs.iter_mut().flatten() {
            t.i0 -= x0;
            t.i1 -= x0;
        }
        Document {
            src,
            ay,
            interp,
            area,
            xs,
            x0,
            x1: x1 - x0,
        }
    }

    /// Samples every device pixel of `part` (∩ the area) of a band and composites it source-over,
    /// through a row cache over just the source columns those pixels read.
    fn paint(
        &self,
        mode: RenderMode,
        part: IntRect,
        y0: i32,
        tw: usize,
        band: &mut [[f32; 4]],
        s: &mut Scratch,
    ) {
        let a = self.area.intersect(&part);
        if a.is_empty() {
            return;
        }
        let Scratch { cache, taps } = s;
        let (lo, hi, xs) = if (a.x, a.w) == (self.area.x, self.area.w) {
            (0, self.x1, &self.xs[..])
        } else {
            let xs = &self.xs[(a.x - self.area.x) as usize..(a.right() - self.area.x) as usize];
            let lo = xs.iter().map(|t| tap_span(t).0).min().unwrap_or(0);
            let hi = xs.iter().map(|t| tap_span(t).1).max().unwrap_or(0);
            taps.clear();
            taps.extend(xs.iter().map(|t| {
                t.map(|t| Tap {
                    i0: t.i0 - lo,
                    i1: t.i1 - lo,
                    f: t.f,
                })
            }));
            (lo, hi, &taps[..])
        };
        cache.reset(self.x0 + lo, hi - lo + 1);
        for y in a.y..a.bottom() {
            let row = &mut band_row(band, y0, tw, y)[a.x as usize..a.right() as usize];
            let ty = self.ay.taps(y, self.interp, 1);
            let (tlo, thi) = tap_span(&ty);
            cache.fill(self.src, tlo, thi);
            for (o, tx) in row.iter_mut().zip(xs) {
                let p = match self.interp {
                    Interp::Nearest => cache.row(ty[0].i0)[tx[0].i0 as usize],
                    Interp::Linear => cache.bilinear(tx[0], ty[0]),
                    Interp::LinearMultisample4x => {
                        let b: [[f32; 4]; 5] =
                            std::array::from_fn(|i| cache.bilinear(tx[i], ty[i]));
                        std::array::from_fn(|ch| {
                            0.5 * b[0][ch] + 0.125 * (b[1][ch] + b[2][ch] + b[3][ch] + b[4][ch])
                        })
                    }
                };
                blend_over(mode, o, p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::surface::Surface;
    use crate::pixel::IntRect;

    fn white(w: u32, h: u32) -> Surface {
        let mut s = Surface::create(w, h);
        s.fill_rect(IntRect::new(0, 0, w, h), [255, 255, 255, 255]);
        s
    }

    fn frame(
        doc: &Surface,
        scale: f64,
        origin: [f64; 2],
        viewport: [u32; 2],
        u: f64,
        dark: bool,
        mode: RenderMode,
    ) -> Vec<u8> {
        let t = ViewTransform {
            scale,
            origin,
            viewport,
            ui_scale: u,
        };
        let style = CanvasStyle::for_scheme(dark, true, None, 0.75, mode);
        let mut target = DisplayTarget::new(viewport[0], viewport[1], mode);
        render_view(doc, 1, &mut MipChain::default(), &t, &style, &mut target);
        target.to_rgba8()
    }

    fn gray(f: &[u8], w: u32, x: u32, y: u32) -> u8 {
        f[((y * w + x) * 4) as usize]
    }

    /// canvas-rendering.md §5 "Shadow": px 1–6 outside the left edge, light theme (#CFCFCF).
    #[test]
    fn shadow_vectors() {
        let doc = white(20, 20);
        let hw = frame(
            &doc,
            1.0,
            [20.0, 20.0],
            [60, 60],
            1.0,
            false,
            RenderMode::Hardware,
        );
        assert_eq!(
            (1..=6)
                .map(|k| gray(&hw, 60, 20 - k, 30))
                .collect::<Vec<_>>(),
            [163, 194, 204, 207, 207, 207]
        );
        let sw = frame(
            &doc,
            1.0,
            [20.0, 20.0],
            [60, 60],
            1.0,
            false,
            RenderMode::Software,
        );
        assert_eq!(
            (1..=6)
                .map(|k| gray(&sw, 60, 20 - k, 30))
                .collect::<Vec<_>>(),
            [122, 179, 201, 206, 207, 207]
        );
        let dark = frame(
            &doc,
            1.0,
            [20.0, 20.0],
            [60, 60],
            1.0,
            true,
            RenderMode::Hardware,
        );
        assert_eq!(
            (1..=6)
                .map(|k| gray(&dark, 60, 20 - k, 30))
                .collect::<Vec<_>>(),
            [96, 54, 48, 48, 48, 48]
        );
        // Corner diagonal (1,1), (2,1), (2,2).
        assert_eq!(
            [
                gray(&hw, 60, 19, 19),
                gray(&hw, 60, 18, 19),
                gray(&hw, 60, 18, 18)
            ],
            [191, 202, 205]
        );
        assert_eq!(
            [
                gray(&sw, 60, 19, 19),
                gray(&sw, 60, 18, 19),
                gray(&sw, 60, 18, 18)
            ],
            [172, 195, 203]
        );
        // u = 1.5: a 7-px band.
        let big = frame(
            &doc,
            1.0,
            [20.0, 20.0],
            [60, 60],
            1.5,
            false,
            RenderMode::Hardware,
        );
        assert_eq!(
            (1..=7)
                .map(|k| gray(&big, 60, 20 - k, 30))
                .collect::<Vec<_>>(),
            [155, 182, 197, 203, 206, 207, 207]
        );
        // No hairline: the first canvas column is the document's white.
        assert_eq!(gray(&hw, 60, 20, 30), 255);
    }

    #[test]
    fn checkerboard_under_transparent_pixels_is_viewport_anchored() {
        let doc = Surface::create(16, 16);
        let f = frame(
            &doc,
            1.0,
            [4.0, 4.0],
            [24, 24],
            1.0,
            false,
            RenderMode::Hardware,
        );
        // Device (4,4) is in square (0,0): dark 143; device (8,4) in square (1,0): light 191.
        assert_eq!(
            (gray(&f, 24, 4, 4), gray(&f, 24, 8, 4), gray(&f, 24, 8, 8)),
            (143, 191, 143)
        );
    }

    /// canvas-rendering.md §5 "Mip count", "Mip choice", "Interpolation", "LMS4x taps".
    #[test]
    fn mips_and_interpolation_choice() {
        assert_eq!(
            [
                mip_count(255, 255),
                mip_count(256, 256),
                mip_count(800, 600),
                mip_count(4000, 3000)
            ],
            [1, 2, 3, 5]
        );
        assert_eq!(
            [1.0, 0.75, 0.5, 0.4, 0.25, 0.1].map(|s| mip_level(s, 3)),
            [0, 0, 1, 1, 2, 2]
        );
        let hw = RenderMode::Hardware;
        assert_eq!(choose_interp(2.0, 800, 800, hw), (0, None, Interp::Nearest));
        assert_eq!(choose_interp(3.0, 800, 800, hw), (0, None, Interp::Nearest));
        assert_eq!(
            choose_interp(1.5, 800, 800, hw),
            (0, Some(2), Interp::LinearMultisample4x)
        );
        assert_eq!(choose_interp(0.5, 800, 800, hw), (1, None, Interp::Linear));
        assert_eq!(
            choose_interp(0.75, 800, 800, hw).2,
            Interp::LinearMultisample4x
        );
        assert_eq!(
            choose_interp(0.3, 800, 800, hw).2,
            Interp::LinearMultisample4x
        );
        assert_eq!(
            choose_interp(0.75, 800, 800, RenderMode::Software).2,
            Interp::Linear
        );
        let [d0, d1, d2, d3] = multisample_offsets(0.75);
        let close =
            |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
        assert!(
            close(d0, [0.1667, 0.5])
                && close(d1, [-0.5, 0.1667])
                && close(d2, [-0.1667, -0.5])
                && close(d3, [0.5, -0.1667])
        );
    }

    /// canvas-rendering.md §5 "NN" and §5 Task 21 "Snap".
    #[test]
    fn integer_zoom_snaps_and_samples_nearest() {
        let t = ViewTransform {
            scale: 2.0,
            origin: [20.6, 0.0],
            viewport: [100, 10],
            ui_scale: 1.0,
        }
        .snapped(false);
        assert_eq!(t.origin[0], 21.0);
        let mut doc = Surface::create(2, 1);
        doc.fill_rect(IntRect::new(0, 0, 1, 1), [255, 0, 0, 255]);
        doc.fill_rect(IntRect::new(1, 0, 1, 1), [0, 0, 255, 255]);
        let f = frame(
            &doc,
            3.0,
            [21.0, 0.0],
            [40, 3],
            1.0,
            false,
            RenderMode::Hardware,
        );
        let px = |x: u32| [f[(x * 4) as usize], f[(x * 4 + 2) as usize]];
        assert_eq!(
            [px(21), px(22), px(23), px(24)],
            [[255, 0], [255, 0], [255, 0], [0, 255]]
        );
    }

    /// Review Focus 1: canvas 100 % maps each image pixel to exactly one device pixel.
    #[test]
    fn view_100_percent_is_one_device_pixel() {
        let mut doc = Surface::create(4, 4);
        for (i, px) in doc.data_mut().chunks_mut(4).enumerate() {
            px.copy_from_slice(&[(i * 16) as u8, 255 - (i * 16) as u8, 7, 255]);
        }
        let f = frame(
            &doc,
            1.0,
            [10.0, 6.0],
            [24, 16],
            1.25,
            false,
            RenderMode::Hardware,
        );
        for y in 0..4u32 {
            for x in 0..4u32 {
                let at = (((6 + y) * 24 + 10 + x) * 4) as usize;
                assert_eq!(
                    &f[at..at + 4],
                    [doc.pixel(x, y)[0], doc.pixel(x, y)[1], 7, 255],
                    "({x},{y})"
                );
            }
        }
    }

    /// §2.2 step 4(c): mips average in linear light in both modes — 1-px black/white stripes at
    /// 50 % (mip 1, k = 1, Linear at texel centres) show linear ½ = sRGB 188, not 128.
    #[test]
    fn mips_average_stripes_in_linear_light() {
        let mut doc = Surface::create(300, 10);
        for (i, px) in doc.data_mut().chunks_mut(4).enumerate() {
            let v = if i % 2 == 0 { 0 } else { 255 };
            px.copy_from_slice(&[v, v, v, 255]);
        }
        for mode in [RenderMode::Hardware, RenderMode::Software] {
            assert_eq!(choose_interp(0.5, 300, 10, mode), (1, None, Interp::Linear));
            let f = frame(&doc, 0.5, [3.0, 2.0], [160, 10], 1.0, false, mode);
            let inside: Vec<u8> = (3..153)
                .flat_map(|x| [gray(&f, 160, x, 2), gray(&f, 160, x, 6)])
                .collect();
            assert!(inside.iter().all(|&g| g == 188), "{mode:?}: {inside:?}");
        }
    }

    /// 701 × 531 noise with every alpha: level 1 and 2 edge tiles use the Linear kernel.
    fn noise(seed: u32) -> Surface {
        let mut doc = Surface::create(701, 531);
        for (i, p) in doc.data_mut().chunks_mut(4).enumerate() {
            let v = (i as u32 ^ seed).wrapping_mul(2654435761);
            p.copy_from_slice(&[v as u8, (v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8]);
        }
        doc
    }

    /// An edit crossing the 512-px tile rows and columns of levels 1 and 2.
    const EDIT: IntRect = IntRect::new(508, 508, 9, 9);

    fn edit(doc: &mut Surface) {
        doc.fill_rect(EDIT, [250, 20, 90, 200]);
    }

    fn pixels(t: &DisplayTarget) -> Vec<[f32; 4]> {
        (0..t.height as i32)
            .flat_map(|y| (0..t.width as i32).map(move |x| (x, y)))
            .map(|(x, y)| t.get(x, y))
            .collect()
    }

    /// Fix round 1: rebuilding only the tiles an edit touched equals a fresh build bit for bit, at
    /// every level and in both modes, without rebuilding whole levels.
    #[test]
    fn mip_tiles_rebuild_like_a_fresh_build() {
        for mode in [RenderMode::Hardware, RenderMode::Software] {
            let mut doc = noise(1);
            let mut chain = MipChain::default();
            for n in 1..3 {
                chain.level(&doc, 1, mode, n);
            }
            edit(&mut doc);
            chain.invalidate(Some(EDIT));
            let dirty: Vec<usize> = chain
                .levels
                .iter()
                .flatten()
                .map(|m| m.dirty.iter().filter(|d| **d).count())
                .collect();
            // Level 1: 3 × 3 tiles of 256 px, 4 touched; level 2: 2 × 2 tiles of 512 px, all 4.
            assert_eq!(dirty, [4, 4]);
            let mut fresh = MipChain::default();
            for n in 1..3 {
                let rebuilt = &chain.level(&doc, 2, mode, n).data;
                assert!(
                    *rebuilt == fresh.level(&doc, 2, mode, n).data,
                    "{mode:?} level {n}"
                );
            }
            assert_eq!(chain.builds(), 2);
            // A revision `invalidate` did not announce drops everything.
            chain.level(&doc, 9, mode, 1);
            assert_eq!(chain.builds(), 3);
        }
    }

    /// Fix round 1: after an edit, re-rendering only `dirty_rect` gives the full render's frame at
    /// NN, fractional, level-0 and mip zooms (Linear edge tiles included), in both modes. The
    /// cases include the widest reach a stress sweep found (2 px past the edit's image at s = ½).
    #[test]
    fn dirty_rect_rerender_matches_full_render() {
        let original = noise(2);
        for mode in [RenderMode::Hardware, RenderMode::Software] {
            for s in [1.0, 2.7, 1.5, 0.75, 0.5, 0.3, 0.13] {
                for k in 0..4 {
                    let mut doc = original.clone();
                    let (e, at) = if k < 3 {
                        let e = IntRect::new(508 + k * 37 % 61, 508 - k * 13 % 29, 1 + k as u32, 2);
                        (e, 512.0)
                    } else {
                        (EDIT, 512.0)
                    };
                    let f = 0.13 + 0.17 * k as f64;
                    let t = ViewTransform {
                        scale: s,
                        origin: [150.0 + f - at * s, 100.0 + 0.7 * f - at * s],
                        viewport: [300, 200],
                        ui_scale: 1.0,
                    };
                    let style = CanvasStyle::for_scheme(false, true, None, 0.75, mode);
                    let mut mips = MipChain::default();
                    let mut target = DisplayTarget::new(300, 200, mode);
                    render_view(&doc, 1, &mut mips, &t, &style, &mut target);
                    doc.fill_rect(e, [250, 20, 90, 200]);
                    mips.invalidate(Some(e));
                    let rect = [e.x, e.y, e.right(), e.bottom()].map(|v| v as f64);
                    target.set_clip(Some(t.dirty_rect(rect)));
                    render_view(&doc, 2, &mut mips, &t, &style, &mut target);
                    let mut fresh = DisplayTarget::new(300, 200, mode);
                    render_view(&doc, 2, &mut MipChain::default(), &t, &style, &mut fresh);
                    assert!(
                        pixels(&target) == pixels(&fresh),
                        "{mode:?} s = {s} k = {k}"
                    );
                }
            }
        }
    }

    /// Task 27r: rendering just some one-row spans (across the surround, shadow, checkerboard and
    /// document, inside a clip) writes exactly the full render's pixels there and nothing else, at
    /// NN, fractional, mip and multisample zooms, in both modes.
    #[test]
    fn span_renders_draw_just_the_spans() {
        let doc = noise(4);
        for mode in [RenderMode::Hardware, RenderMode::Software] {
            for s in [1.0, 2.7, 0.75, 0.3] {
                let t = ViewTransform {
                    scale: s,
                    origin: [40.6 - 300.0 * s, 31.2 - 260.0 * s],
                    viewport: [160, 90],
                    ui_scale: 1.25,
                };
                let style = CanvasStyle::for_scheme(true, true, None, 0.75, mode);
                let mut full = DisplayTarget::new(160, 90, mode);
                render_view(&doc, 1, &mut MipChain::default(), &t, &style, &mut full);
                let spans: Vec<IntRect> = (0..90)
                    .step_by(3)
                    .flat_map(|y| {
                        let x = (y * 37 % 150) - 20;
                        [IntRect::new(x, y, 9, 1), IntRect::new(x + 30, y, 70, 1)]
                    })
                    .collect();
                let mut part = DisplayTarget::new(160, 90, mode);
                (0..90).for_each(|y| (0..160).for_each(|x| part.set(x, y, [0.25, 0.5, 0.75, 1.0])));
                let marker = part.get(0, 0);
                part.set_clip(Some(IntRect::new(0, 4, 150, 80)));
                let mut mips = MipChain::default();
                render_view_in(&doc, 1, &mut mips, &t, &style, &mut part, Some(&spans));
                let clip = part.clip();
                for y in 0..90 {
                    for x in 0..160 {
                        let inside = clip.contains(x, y) && spans.iter().any(|r| r.contains(x, y));
                        let want = if inside { full.get(x, y) } else { marker };
                        assert!(part.get(x, y) == want, "{mode:?} s = {s} at ({x}, {y})");
                    }
                }
            }
        }
    }

    /// Rendering in 7-row strips (other band boundaries than the parallel bands') equals one pass.
    #[test]
    fn banding_never_changes_a_pixel() {
        let doc = noise(3);
        for (s, mode) in [(1.5, RenderMode::Hardware), (0.3, RenderMode::Software)] {
            let t = ViewTransform {
                scale: s,
                origin: [-40.3, -21.8],
                viewport: [160, 90],
                ui_scale: 1.0,
            };
            let style = CanvasStyle::for_scheme(true, true, None, 0.75, mode);
            let mut whole = DisplayTarget::new(160, 90, mode);
            render_view(&doc, 1, &mut MipChain::default(), &t, &style, &mut whole);
            let mut strips = DisplayTarget::new(160, 90, mode);
            let mut mips = MipChain::default();
            for y in (0..90).step_by(7) {
                strips.set_clip(Some(IntRect::new(0, y, 160, 7)));
                render_view(&doc, 1, &mut mips, &t, &style, &mut strips);
            }
            assert!(pixels(&strips) == pixels(&whole), "{mode:?} s = {s}");
        }
    }

    /// Review Focus 3: extreme zooms on a big document neither panic nor rebuild mips per frame.
    #[test]
    fn view_render_extreme_zooms() {
        let doc = white(2048, 1536);
        let mut mips = MipChain::default();
        let style = CanvasStyle::for_scheme(true, true, None, 0.75, RenderMode::Hardware);
        for _ in 0..2 {
            for scale in [0.01, 0.5, 1.0, 37.5, 100.0] {
                let t = ViewTransform {
                    scale,
                    origin: [-17.0 * scale, -9.0 * scale],
                    viewport: [200, 150],
                    ui_scale: 1.0,
                };
                let mut target = DisplayTarget::new(200, 150, RenderMode::Hardware);
                render_view(&doc, 7, &mut mips, &t, &style, &mut target);
                assert!(target.to_rgba8().chunks(4).all(|p| p[3] == 255));
            }
        }
        // n = 5 for 2048 × 1536: levels 4 (s = 0.01, clamped to n − 1) and 1 (s = 0.5), once each.
        assert_eq!(mips.builds(), 2);
    }

    #[test]
    fn hidpi_one_and_a_half() {
        assert_eq!(crate::render::checker::checker_square(1.5, true), 8);
        let doc = white(20, 20);
        let f = frame(
            &doc,
            1.0,
            [20.0, 20.0],
            [60, 60],
            1.5,
            false,
            RenderMode::Hardware,
        );
        // The shadow band is 7 px: px 8 outside the edge is the bare surround.
        assert_eq!((gray(&f, 60, 13, 30), gray(&f, 60, 12, 30)), (207, 207));
        assert_ne!(gray(&f, 60, 19, 30), 207);
    }
}
