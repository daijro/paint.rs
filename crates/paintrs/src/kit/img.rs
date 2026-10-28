//! Chromium's `<img>` paint path (software raster, `--disable-gpu`) for the 64x64 icon PNGs
//! that CSS draws at 20 px. Ports what Blink/cc/Skia do, as measured pixel-exact against the
//! shell-no-forms references (gear, file-new, paintbrush; six disabled icons):
//! 1. decode, premultiply (`round(c * a / 255)`);
//! 2. cc's software decode cache scales to the smallest-not-smaller `SkMipmap` level
//!    (largest level >= the target size; 2x2 box, truncating shifts);
//! 3. Skia's legacy `SkBitmapProcState` bilinear: inverse `RectToRect` matrix in f32,
//!    32.32 fractional coordinates minus half a texel, 4-bit weights, `>> 8`; each row maps
//!    its first pixel in f32 and then steps x by the 32.32 inverse scale (`filter_scale`);
//!    coordinates are local to cc's raster tile: 256 px tiles with 1 px borders, i.e. origins
//!    at multiples of 254 (the only tiling consistent with all 19 icons in the references;
//!    an image over a tile edge is treated as wholly in the tile of its top-left pixel);
//! 4. src-over the backdrop with `SkAlphaMulQ`: `src + (dst * (256 - a) >> 8)`.
//!
//! `.menu-item[disabled] img { filter: grayscale(100%) }` rasters the image into a filter layer
//! whose origin is the image's corner (the deselect icon over a tile edge pins this), then runs
//! one f32 raster-pipeline pass: unpremultiply, luma row (0.2126, 0.7152, 0.0722), premultiply,
//! src-over, round.
//! Reference: paintdotjs @ b4e6fc5 (css/menu.css `.icon-item .icon`, `.menu-item[disabled] img`)
use egui::{
    Color32, ColorImage, Context, Id, Painter, Rect, TextureHandle, TextureId, TextureOptions,
};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Filter {
    None,
    /// CSS `filter: grayscale(100%)`.
    Grayscale,
    /// CSS opacity on an opaque button group, over the row beneath the group.
    Opacity {
        alpha: u8,
        background: Color32,
    },
}

/// Everything the rendered pixels depend on: the raster position only through its tile-local
/// remainder (none at all inside a filter layer), so moving an icon by whole tiles, e.g. a
/// right-aligned one across window widths, reuses the texture.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Key {
    name: String,
    /// Raster-local destination origin in device pixels (see `local_origin`).
    origin: [i32; 2],
    size: [i32; 2],
    backdrop: Backdrop,
    filter: Filter,
    selected: crate::icon_pack::IconPackId,
    dark: bool,
    ppp: u32,
}

/// What lies under the image: one color, or one per pixel (row-major, the image's size), e.g.
/// an inset box-shadow ring under a selected icon.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum Backdrop {
    Flat([u8; 3]),
    Pixels(Arc<Vec<[u8; 3]>>),
}

impl Backdrop {
    fn at(&self, i: usize) -> [u8; 3] {
        match self {
            Backdrop::Flat(c) => *c,
            Backdrop::Pixels(px) => px[i],
        }
    }
}

/// A texture, its pixels and the last pass that used it.
type Kept = (TextureHandle, Arc<ColorImage>, u64);

/// Textures by key, with the pass that last used each; entries unused in the previous pass are
/// dropped at the first use of a new pass.
#[derive(Clone, Default)]
struct Cache {
    /// Texture, its pixels (for `kit::shadow`'s backdrop replay), last pass used.
    entries: HashMap<Key, Kept>,
    /// `paint_pixels`' textures by their size and pixels, likewise.
    pixels: HashMap<([usize; 2], Vec<Color32>), Kept>,
    pass: u64,
}

impl Cache {
    /// Starts pass `pass`: drops what the previous pass did not use.
    fn begin(&mut self, pass: u64) {
        if self.pass != pass {
            self.pass = pass;
            self.entries.retain(|_, (_, _, used)| *used + 1 >= pass);
            self.pixels.retain(|_, (_, _, used)| *used + 1 >= pass);
        }
    }
}

/// Paints `image`, one texel per device pixel, with its top-left at `at` (points on a
/// device-pixel corner) as one textured quad (transparent texels leave what is under them), in
/// place of a shape per pixel. Its texture is kept while a pass paints it, and
/// [`texture_pixels`] has its pixels (`kit::shadow` replays it).
pub fn paint_pixels(painter: &Painter, at: egui::Pos2, image: ColorImage) {
    let ctx = painter.ctx();
    let pass = ctx.cumulative_pass_nr();
    let key = (image.size, image.pixels);
    let hit = ctx.data_mut(|d| {
        let cache = d.get_temp_mut_or_default::<Cache>(cache_id());
        cache.begin(pass);
        cache.pixels.get_mut(&key).map(|(t, _, used)| {
            *used = pass;
            t.clone()
        })
    });
    let tex = hit.unwrap_or_else(|| {
        let image = Arc::new(ColorImage::new(key.0, key.1.clone()));
        let tex = ctx.load_texture("pixels", image.clone(), TextureOptions::NEAREST);
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Cache>(cache_id())
                .pixels
                .insert(key, (tex.clone(), image, pass))
        });
        tex
    });
    let size = tex.size_vec2() / painter.pixels_per_point();
    painter.image(
        tex.id(),
        Rect::from_min_size(at, size),
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

fn cache_id() -> Id {
    Id::unique("paintrs.kit.img")
}

/// The origin the raster math sees: tile-local without a filter, the layer origin with one.
fn local_origin(x: i32, y: i32, filter: Filter) -> [i32; 2] {
    match filter {
        Filter::None => [tile_local(x), tile_local(y)],
        Filter::Grayscale => [0, 0],
        Filter::Opacity { .. } => [x, y],
    }
}

/// Paints icon `name` (an `assets/icons` file name) into `rect` (points, already pixel-snapped)
/// over the flat `backdrop` color, as Chromium paints the `<img>` in the root layer.
pub fn paint_icon(painter: &Painter, name: &str, rect: Rect, backdrop: Color32, filter: Filter) {
    paint_icon_in_layer(painter, name, rect, backdrop, filter, egui::Pos2::ZERO);
}

/// `paint_icon` for an image rastered in a composited layer whose origin is `layer` (points,
/// pixel-snapped). Drop menus get their own layer from css/menu.css `.drop-menu { animation:
/// fadeIn 0.1s }` (an opacity animation), so their icons' raster coordinates are popup-local;
/// the root-layer tiling mispredicts the menu icons at x 206 and 267, the popup origin fits all.
pub fn paint_icon_in_layer(
    painter: &Painter,
    name: &str,
    rect: Rect,
    backdrop: Color32,
    filter: Filter,
    layer: egui::Pos2,
) {
    let flat = Backdrop::Flat([backdrop.r(), backdrop.g(), backdrop.b()]);
    paint(painter, name, None, rect, flat, filter, layer);
}

/// `paint_icon` for an image the app draws itself (a `<canvas>.toDataURL()` shown as an
/// `<img>`, e.g. the document strip's dirty star): `key` names `src`'s content.
pub fn paint_generated(
    painter: &Painter,
    key: &str,
    src: &Arc<image::RgbaImage>,
    rect: Rect,
    backdrop: Color32,
) {
    let flat = Backdrop::Flat([backdrop.r(), backdrop.g(), backdrop.b()]);
    let src = Some(src.clone());
    paint(
        painter,
        key,
        src,
        rect,
        flat,
        Filter::None,
        egui::Pos2::ZERO,
    );
}

/// `paint_icon` over a per-pixel `backdrop` (row-major, the snapped rect's device size).
pub fn paint_icon_over(painter: &Painter, name: &str, rect: Rect, backdrop: &[Color32]) {
    let px = backdrop.iter().map(|c| [c.r(), c.g(), c.b()]).collect();
    let b = Backdrop::Pixels(Arc::new(px));
    paint(painter, name, None, rect, b, Filter::None, egui::Pos2::ZERO);
}

/// `src`: the image's pixels when the app generates them, else the `name` asset.
fn paint(
    painter: &Painter,
    name: &str,
    src: Option<Arc<image::RgbaImage>>,
    rect: Rect,
    backdrop: Backdrop,
    filter: Filter,
    layer: egui::Pos2,
) {
    let ctx = painter.ctx();
    let ppp = painter.pixels_per_point();
    let (x, y) = (
        ((rect.min.x - layer.x) * ppp).round() as i32,
        ((rect.min.y - layer.y) * ppp).round() as i32,
    );
    let key = Key {
        name: name.to_string(),
        origin: local_origin(x, y, filter),
        size: [
            (rect.width() * ppp).round() as i32,
            (rect.height() * ppp).round() as i32,
        ],
        backdrop,
        filter,
        selected: crate::assets::Assets::selected(ctx),
        dark: matches!(
            crate::theme::current(ctx).scheme,
            crate::theme::Scheme::Dark | crate::theme::Scheme::Blue
        ),
        ppp: ppp.to_bits(),
    };
    let Some(texture) = texture(ctx, key, src) else {
        return;
    };
    painter.image(
        texture.id(),
        rect,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// Drops the cached renderings of icon `name` (an icon pack replaced its pixels).
pub fn forget_icon(ctx: &Context, name: &str) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(cache_id())
            .entries
            .retain(|k, _| k.name != name)
    });
}

/// Drops every cached rendering (the icon pack changed).
pub fn forget_all(ctx: &Context) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(cache_id())
            .entries
            .clear()
    });
}

fn texture(ctx: &Context, key: Key, src: Option<Arc<image::RgbaImage>>) -> Option<TextureHandle> {
    let pass = ctx.cumulative_pass_nr();
    let hit = ctx.data_mut(|d| {
        let cache = d.get_temp_mut_or_default::<Cache>(cache_id());
        cache.begin(pass);
        cache.entries.get_mut(&key).map(|(t, _, used)| {
            *used = pass;
            t.clone()
        })
    });
    if hit.is_some() {
        return hit;
    }
    let src = match src {
        Some(src) => src,
        None => Arc::new(crate::assets::Assets::icon_image_sized(
            ctx,
            &key.name,
            key.size[0].max(1) as u32,
            key.size[1].max(1) as u32,
        )?),
    };
    let ([x, y], [w, h]) = (key.origin, key.size);
    let rgba = render(&src, x, y, w as u32, h as u32, &key.backdrop, key.filter);
    let image = Arc::new(ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        &rgba,
    ));
    let tex = ctx.load_texture(
        format!("img/{}", key.name),
        image.clone(),
        TextureOptions::NEAREST,
    );
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(cache_id())
            .entries
            .insert(key, (tex.clone(), image, pass))
    });
    Some(tex)
}

/// The pixels of an icon texture this module rendered, or of a `paint_pixels` texture.
pub fn texture_pixels(ctx: &Context, id: TextureId) -> Option<Arc<ColorImage>> {
    ctx.data_mut(|d| {
        let cache = d.get_temp_mut_or_default::<Cache>(cache_id());
        (cache.entries.values().chain(cache.pixels.values()))
            .find(|(t, ..)| t.id() == id)
            .map(|(_, img, _)| img.clone())
    })
}

/// Premultiplied RGBA8 image.
struct Premul {
    w: u32,
    h: u32,
    px: Vec<[u32; 4]>,
}

fn premultiply(img: &image::RgbaImage) -> Premul {
    let px = img
        .pixels()
        .map(|p| {
            let a = p.0[3] as u32;
            let m = |c: u8| (c as u32 * a + 127) / 255;
            [m(p.0[0]), m(p.0[1]), m(p.0[2]), a]
        })
        .collect();
    Premul {
        w: img.width(),
        h: img.height(),
        px,
    }
}

/// One 2x2 box level (`SkMipmap` Downsample_2_2, truncating).
fn halve(src: &Premul) -> Premul {
    let (w, h) = ((src.w / 2).max(1), (src.h / 2).max(1));
    let at = |x: u32, y: u32| src.px[(y.min(src.h - 1) * src.w + x.min(src.w - 1)) as usize];
    let mut px = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let q = [
                at(2 * x, 2 * y),
                at(2 * x + 1, 2 * y),
                at(2 * x, 2 * y + 1),
                at(2 * x + 1, 2 * y + 1),
            ];
            px.push(std::array::from_fn(|c| {
                (q[0][c] + q[1][c] + q[2][c] + q[3][c]) >> 2
            }));
        }
    }
    Premul { w, h, px }
}

/// cc software raster tile stride (256 px tiles, 1 px border texels).
const TILE_STRIDE: i32 = 254;

/// A device coordinate in its raster tile's coordinates. cc tiles are 256 px with 1 px border
/// texels at a 254 px stride: tile `i` owns `[254 i + 1, 254 (i + 1)]` (tile 0 from 0), so a
/// tile-local coordinate runs 1..=254 (the spin-button dither at x 508 pins the ownership).
pub fn tile_local(v: i32) -> i32 {
    if v <= 0 {
        v
    } else {
        v - TILE_STRIDE * ((v - 1) / TILE_STRIDE)
    }
}

/// `SkBitmapProcStateAutoMapper` for one axis: device pixel center through the f32 inverse
/// matrix, as 32.32 fractional int with the bilerp half-texel bias.
fn fractional_coord(device: i32, inv_scale: f32, inv_translate: f32) -> i64 {
    let pt = (device as f32 + 0.5) * inv_scale + inv_translate;
    let fractional = (pt * 4_294_967_296.0f32) as i64;
    fractional - (1 << 31)
}

/// 16.16 fixed (`SkFractionalIntToFixed`).
fn fixed_coord(device: i32, inv_scale: f32, inv_translate: f32) -> i64 {
    fractional_coord(device, inv_scale, inv_translate) >> 16
}

fn render(
    img: &image::RgbaImage,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    backdrop: &Backdrop,
    filter: Filter,
) -> Vec<u8> {
    let mut src = premultiply(img);
    while src.w / 2 >= w && src.h / 2 >= h && src.w > 1 {
        src = halve(&src);
    }
    // (x, y) is raster-local (`local_origin`); SkMatrix::RectToRect(src, dst) inverted in f32.
    let (sx, sy) = (w as f32 / src.w as f32, h as f32 / src.h as f32);
    let (ix, iy) = (1.0f32 / sx, 1.0f32 / sy);
    let step = (ix * 4_294_967_296.0f32) as i64;
    let tap = |xx: i64, yy: i64| {
        let xx = xx.clamp(0, src.w as i64 - 1) as u32;
        let yy = yy.clamp(0, src.h as i64 - 1) as u32;
        src.px[(yy * src.w + xx) as usize]
    };
    // Raster tiles: a pixel past the tile's last owned column rasters in the next tile's
    // coordinates (a filter layer has one origin), each tile stepping x from its first texel.
    let tiled = filter == Filter::None;
    let split = |origin: i32, len: i32| -> i32 {
        if tiled && origin <= TILE_STRIDE && origin + len > TILE_STRIDE + 1 {
            TILE_STRIDE + 1 - origin
        } else {
            len
        }
    };
    let (split_x, split_y) = (split(x, w as i32), split(y, h as i32));
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for j in 0..h as i32 {
        let (y, j) = if j >= split_y {
            (y - TILE_STRIDE, j)
        } else {
            (y, j)
        };
        let ty = -(y as f32) * iy;
        let fy = fixed_coord(y + j, iy, ty);
        let (y0, wy) = (fy >> 16, ((fy >> 12) & 0xF) as u32);
        for i in 0..w as i32 {
            // The next tile rasters from its own border texel (local 0, one column before the
            // first pixel it owns) and steps x from there.
            let (x, start) = if i >= split_x {
                (x - TILE_STRIDE, split_x - 1)
            } else {
                (x, 0)
            };
            let tx = -(x as f32) * ix;
            let fx0 = fractional_coord(x + start, ix, tx);
            let fx = (fx0 + (i - start) as i64 * step) >> 16;
            let (x0, wx) = (fx >> 16, ((fx >> 12) & 0xF) as u32);
            let taps = [
                (tap(x0, y0), (16 - wx) * (16 - wy)),
                (tap(x0 + 1, y0), wx * (16 - wy)),
                (tap(x0, y0 + 1), (16 - wx) * wy),
                (tap(x0 + 1, y0 + 1), wx * wy),
            ];
            let s: [u32; 4] =
                std::array::from_fn(|c| taps.iter().map(|(p, k)| p[c] * k).sum::<u32>() >> 8);
            let backdrop = backdrop.at((j * w as i32 + i) as usize);
            let rgb = match filter {
                Filter::None | Filter::Opacity { .. } => std::array::from_fn::<u8, 3, _>(|c| {
                    (s[c] + ((backdrop[c] as u32 * (256 - s[3])) >> 8)) as u8
                }),
                Filter::Grayscale => grayscale_over(s, backdrop),
            };
            let rgb = if let Filter::Opacity { alpha, background } = filter {
                let c = crate::kit::px::blend(
                    background,
                    Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                    alpha,
                );
                [c.r(), c.g(), c.b()]
            } else {
                rgb
            };
            out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    out
}

fn grayscale_over(s: [u32; 4], backdrop: [u8; 3]) -> [u8; 3] {
    let a = (s[3] as f64 / 255.0) as f32;
    let inv = if a > 0.0 { 1.0 / a } else { 0.0 };
    let u = |c: usize| (s[c] as f64 / 255.0) as f32 * inv;
    let g = (u(0) * 0.2126f32 + u(1) * 0.7152f32 + u(2) * 0.0722f32).clamp(0.0, 1.0);
    std::array::from_fn(|c| {
        let d = (backdrop[c] as f64 / 255.0) as f32;
        let v = (g * a + d * (1.0 - a)) * 255.0;
        (v + 0.5).floor() as u8
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icon_in_disabled_button_composites_the_whole_group_once() {
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 128]));
        let pixels = render(
            &image,
            4,
            4,
            1,
            1,
            &Backdrop::Flat([253; 3]),
            Filter::Opacity {
                alpha: 140,
                background: Color32::from_rgb(30, 30, 30),
            },
        );
        assert_eq!(pixels, [152, 82, 82, 255]);
    }

    fn paint(ctx: &Context, name: &str, x: f32) {
        let mut out = ctx.run_pass(egui::RawInput::default(), |ctx| {
            let p = ctx.layer_painter(egui::LayerId::background());
            let r = Rect::from_min_size(egui::pos2(x, 13.0), egui::vec2(20.0, 20.0));
            paint_icon(&p, name, r, Color32::from_gray(0x20), Filter::None);
        });
        out.textures_delta.clear(); // no renderer in unit tests
    }

    fn cached(ctx: &Context, name: &str) -> Vec<egui::TextureId> {
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Cache>(cache_id())
                .entries
                .iter()
                .filter(|(k, _)| k.name == name)
                .map(|(_, (t, ..))| t.id())
                .collect()
        })
    }

    #[test]
    fn positions_a_tile_apart_share_one_texture() {
        let ctx = Context::default();
        let name = "menu_help_icon.png";
        paint(&ctx, name, 1254.0);
        paint(&ctx, name, 1254.0 - TILE_STRIDE as f32);
        assert_eq!(cached(&ctx, name).len(), 1);
        paint(&ctx, name, 1255.0);
        assert_eq!(
            cached(&ctx, name).len(),
            2,
            "still used in the previous pass"
        );
        paint(&ctx, name, 1255.0);
        assert_eq!(
            cached(&ctx, name).len(),
            1,
            "unused in the previous pass: evicted"
        );
    }

    #[test]
    fn icon_override_replaces_the_cached_rendering() {
        let ctx = Context::default();
        let name = "menu_help_icon.png";
        paint(&ctx, name, 7.0);
        let before = cached(&ctx, name);
        let mut png = vec![];
        image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        crate::assets::Assets::set_icon_override(&ctx, name, png);
        assert!(cached(&ctx, name).is_empty());
        paint(&ctx, name, 7.0);
        let after = cached(&ctx, name);
        assert_eq!(after.len(), 1);
        assert_ne!(after, before);
    }

    #[test]
    fn fixed_coord_rounds_half_rows_like_skia_f32() {
        // Gear icon at device y = 13, 32 -> 20 px: row 7 maps to 11.5 in f64 but Skia's f32
        // matrix lands just below it, so the weight is 7, not 8 (shell-no-forms reference).
        let s = 20.0f32 / 32.0;
        let (iy, ty) = (1.0 / s, -13.0 * (1.0 / s));
        let f = fixed_coord(13 + 7, iy, ty);
        assert_eq!((f >> 16, (f >> 12) & 0xF), (11, 7));
    }

    #[test]
    fn x_steps_in_fixed_point_from_the_row_start() {
        // Open icon at device x = 34 (tile-local 34): column 17 maps to a weight of 7 when
        // stepped from the row start, where per-pixel f32 mapping gives 8 (reference: 7).
        let s = 20.0f32 / 32.0;
        let ix = 1.0 / s;
        let tx = -34.0 * ix;
        let step = (ix * 4_294_967_296.0f32) as i64;
        let stepped = (fractional_coord(34, ix, tx) + 17 * step) >> 16;
        assert_eq!((stepped >> 12) & 0xF, 7);
        assert_eq!((fixed_coord(34 + 17, ix, tx) >> 12) & 0xF, 8);
    }

    #[test]
    fn halve_truncates() {
        let p = Premul {
            w: 2,
            h: 2,
            px: vec![[1, 1, 1, 3], [0, 0, 0, 0], [0, 0, 0, 0], [2, 2, 2, 2]],
        };
        assert_eq!(halve(&p).px, vec![[0, 0, 0, 1]]);
    }
}
