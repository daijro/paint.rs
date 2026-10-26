//! Port of: js/src/engine/AppView.js (setCursorImg, cleanCursorImage, cursorCss),
//! js/src/interface/item/menu/action/ActionItem.js (getIconPath)
//! Reference: paintdotjs @ b4e6fc5

use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use std::collections::HashMap;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_assets.rs"));
}

type Cache = HashMap<String, TextureHandle>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSource {
    UserPack,
    PaintRs,
}
#[derive(Clone, Default)]
struct UserPack(std::collections::BTreeMap<String, std::sync::Arc<[u8]>>);

#[derive(Clone)]
pub struct Cursor {
    pub texture: TextureHandle,
    /// In texture pixels.
    pub hotspot: [u32; 2],
    /// Texture pixels per point: the device scale for an SVG cursor, 1 for a PNG one (CSS
    /// `cursor: url()` draws one image pixel per point).
    pub scale: f32,
}

/// Unit handle for the texture cache and icon-pack overrides, both kept in `ctx.data`.
#[derive(Default)]
pub struct Assets;

#[derive(Clone, Default)]
struct SvgImages(HashMap<(String, u32, bool, crate::icon_pack::IconPackId), image::RgbaImage>);

fn dark_theme(ctx: &Context) -> bool {
    matches!(
        crate::theme::current(ctx).scheme,
        crate::theme::Scheme::Dark | crate::theme::Scheme::Blue
    )
}

fn svg_bytes(name: &str, dark: bool) -> Option<&'static [u8]> {
    let stem = name.strip_suffix(".png")?;
    let (stem, dark) = match stem.strip_suffix("_dark") {
        Some(s) => (s, true),
        None => (stem, dark),
    };
    if dark {
        lookup(embedded::ICON_SVGS, &format!("{stem}.dark.svg"))
            .or_else(|| lookup(embedded::ICON_SVGS, &format!("{stem}.svg")))
    } else {
        lookup(embedded::ICON_SVGS, &format!("{stem}.svg"))
    }
}

fn raster_svg(bytes: &[u8], ppp: f32) -> Option<image::RgbaImage> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = if ppp.is_finite() {
        ppp.clamp(0.25, 8.0)
    } else {
        1.0
    };
    let w = (size.width() * scale).round().clamp(1.0, 512.0) as u32;
    let h = (size.height() * scale).round().clamp(1.0, 512.0) as u32;
    let transform =
        resvg::tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    render_svg(&tree, w, h, transform)
}

/// `tree` drawn with `transform` into a transparent `w` × `h` px image, straight RGBA.
fn render_svg(
    tree: &resvg::usvg::Tree,
    w: u32,
    h: u32,
    transform: resvg::tiny_skia::Transform,
) -> Option<image::RgbaImage> {
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(tree, transform, &mut pixmap.as_mut());
    let mut rgba = pixmap.take();
    // tiny-skia stores premultiplied RGBA; the app's CPU resampler expects straight RGBA.
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = pixel[3] as u32;
        for channel in &mut pixel[..3] {
            *channel = ((*channel as u32 * 255 + alpha / 2)
                .checked_div(alpha)
                .unwrap_or(0))
            .min(255) as u8;
        }
    }
    image::RgbaImage::from_raw(w, h, rgba)
}

/// The Shapes reset button's icon for pdn-core's nubs (drawn 16 × 16 DIP): `reset_icon.svg` at
/// 32 px (crisp at 2×).
pub fn reset_nub_icon() -> Option<resvg::tiny_skia::Pixmap> {
    let svg = svg_bytes("reset_icon.png", false)?;
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(32, 32)?;
    let scale = 32.0 / tree.size().width();
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(pixmap)
}

/// `assets/app-icon.svg` (NOTICE.md; 454 × 329) fitted undistorted and centred on a transparent
/// `side` × `side` px square: the OS window icon, the committed `assets/app-icon.png` (macOS
/// bundle) and the title-bar and About artwork ([`Assets::app_artwork`]).
pub fn app_icon(side: u32) -> image::RgbaImage {
    let svg = include_bytes!("../../../assets/app-icon.svg");
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default())
        .expect("assets/app-icon.svg");
    let (w, h) = (tree.size().width(), tree.size().height());
    let scale = side as f32 / w.max(h);
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(
        (side as f32 - w * scale) / 2.0,
        (side as f32 - h * scale) / 2.0,
    );
    render_svg(&tree, side, side, transform).expect("app icon side")
}

#[derive(Clone, Default)]
struct Overrides(HashMap<String, Vec<u8>>);

/// The interface images an icon pack supplies under `assets/images/` (every other name is under
/// `assets/icons/`).
const UI_IMAGES: [&str; 5] = [
    "anchor_chooser_control_anchor_image.png",
    "image_strip_scroll_left_arrow.png",
    "image_strip_scroll_right_arrow.png",
    "tool_bar_image_list_menu_open_button.png",
    "update_prompt_task_dialog_task_image.png",
];

fn lookup(table: &'static [(&'static str, &'static [u8])], name: &str) -> Option<&'static [u8]> {
    table
        .binary_search_by_key(&name, |(n, _)| n)
        .ok()
        .map(|i| table[i].1)
}

/// A bundled font file by its `assets/font` path (`"inter/Inter-Variable.ttf"`): Noto Sans and
/// Noto Sans Mono everywhere (pdn-core's statics), the other faces in native builds only, with
/// their hinting instructions emptied (build.rs).
pub fn embedded_font(path: &str) -> Option<&'static [u8]> {
    use pdn_core::canvas::text::{NOTO_SANS, NOTO_SANS_MONO};
    match path {
        "noto-sans/NotoSans-Regular.ttf" => Some(NOTO_SANS),
        "noto-sans-mono/NotoSansMono-Regular.ttf" => Some(NOTO_SANS_MONO),
        _ => lookup(embedded::FONTS, path),
    }
}

/// `assets/icons-svg/cursors/manifest.json`: each cursor's hotspot in its SVG's 32 × 32 units.
fn cursor_hotspot(name: &str) -> Option<[f32; 2]> {
    static HOTSPOTS: std::sync::LazyLock<HashMap<String, [f32; 2]>> =
        std::sync::LazyLock::new(|| {
            serde_json::from_str(include_str!(
                "../../../assets/icons-svg/cursors/manifest.json"
            ))
            .expect("cursor manifest")
        });
    HOTSPOTS.get(name).copied()
}

fn color_image(rgba: &image::RgbaImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    )
}

/// `cleanCursorImage`: the red pixels on the last column/row encode the hotspot; they are made
/// transparent and the metadata row and column are cropped away.
fn try_decode_cursor(png: &[u8]) -> Option<(ColorImage, [u32; 2])> {
    decode_cursor_rgba(png).map(|(img, hotspot)| (color_image(&img), hotspot))
}

/// [`try_decode_cursor`] as straight RGBA.
fn decode_cursor_rgba(png: &[u8]) -> Option<(image::RgbaImage, [u32; 2])> {
    let mut img = pdn_core::asset_pack::decode_image(png).ok()?;
    let (w, h) = img.dimensions();
    let (mut hx, mut hy) = (None, None);
    for (x, y, p) in img.enumerate_pixels_mut() {
        if p.0[0] == 255 && p.0[1] == 0 && p.0[2] == 0 && p.0[3] != 0 {
            if x == w - 1 {
                hy = Some(y);
            }
            if y == h - 1 {
                hx = Some(x);
            }
            p.0[3] = 0;
        }
    }
    let meta = hx.is_some() && hy.is_some();
    let (cw, ch) = (w - meta as u32, h - meta as u32);
    if cw == 0 || ch == 0 {
        return None;
    }
    let clean = image::imageops::crop_imm(&img, 0, 0, cw, ch).to_image();
    let hotspot = match (hx, hy) {
        (Some(x), Some(y)) => [x, y],
        _ => [cw / 2, ch / 2],
    };
    Some((clean, hotspot))
}

/// Whether OS cursor bitmaps are one pixel per point rather than per device pixel. winit's macOS
/// backend sizes the `NSImage` as the bitmap's pixels in points, so a device-resolution bitmap
/// would show twice its size on a Retina display; one pixel per point is the right size there,
/// softer. Windows, X11, Wayland (`wp_viewporter`) and the web's `image-set` take device pixels.
const CURSOR_IN_POINTS: bool = cfg!(target_os = "macos");

/// OS cursor bitmaps by [`Assets::native_cursor`]'s key (`None`: the cursor can't be resolved).
#[derive(Clone, Default)]
struct NativeCursors(HashMap<String, Option<egui::CustomCursorImage>>);

fn native_cursors_id() -> egui::Id {
    egui::Id::unique("paintrs_native_cursors")
}

impl Assets {
    pub fn selected(ctx: &Context) -> crate::icon_pack::IconPackId {
        ctx.data(|d| {
            d.get_temp::<crate::icon_pack::IconPackId>(egui::Id::unique(
                "paintrs_selected_icon_pack",
            ))
        })
        .unwrap_or_default()
    }

    pub fn select(ctx: &Context, id: crate::icon_pack::IconPackId) {
        ctx.data_mut(|d| {
            d.insert_temp(egui::Id::unique("paintrs_selected_icon_pack"), id);
            d.get_temp_mut_or_default::<Cache>(egui::Id::unique("paintrs_textures"))
                .clear();
            d.get_temp_mut_or_default::<SvgImages>(egui::Id::unique("paintrs_svg_images"))
                .0
                .clear();
            d.get_temp_mut_or_default::<NativeCursors>(native_cursors_id())
                .0
                .clear();
        });
        crate::kit::img::forget_all(ctx);
    }

    fn svg_image(ctx: &Context, name: &str) -> Option<image::RgbaImage> {
        let ppp = ctx.pixels_per_point();
        let dark = dark_theme(ctx);
        let key = (name.to_owned(), ppp.to_bits(), dark, Self::selected(ctx));
        let id = egui::Id::unique("paintrs_svg_images");
        if let Some(image) = ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<SvgImages>(id)
                .0
                .get(&key)
                .cloned()
        }) {
            return Some(image);
        }
        let image = raster_svg(svg_bytes(name, dark)?, ppp)?;
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<SvgImages>(id)
                .0
                .insert(key, image.clone());
        });
        Some(image)
    }

    /// The title-bar icon and About/Welcome banner artwork, `side` device px square: [`app_icon`]
    /// rendered at `side` (R213, R221).
    pub fn app_artwork(ctx: &Context, side: u32) -> std::sync::Arc<image::RgbaImage> {
        let id = egui::Id::unique(("paintrs_app_icon", side));
        if let Some(image) = ctx.data(|d| d.get_temp(id)) {
            return image;
        }
        let image = std::sync::Arc::new(app_icon(side));
        ctx.data_mut(|d| d.insert_temp(id, image.clone()));
        image
    }

    /// Original bundled paint.rs icon pixels for installed-pack previews.
    pub fn paint_rs_icon_image(ctx: &Context, name: &str) -> Option<image::RgbaImage> {
        Self::svg_image(ctx, name)
    }

    /// AssetPackManager.activate: replace every user path and invalidate all live asset renderings.
    pub fn set_pack(ctx: &Context, pack: Option<pdn_core::asset_pack::AssetPack>) {
        ctx.data_mut(|d| {
            d.insert_temp(
                egui::Id::unique("paintrs_user_pack"),
                UserPack(
                    pack.map(|p| p.files.into_iter().map(|(k, v)| (k, v.into())).collect())
                        .unwrap_or_default(),
                ),
            );
            d.get_temp_mut_or_default::<Cache>(egui::Id::unique("paintrs_textures"))
                .clear();
            d.get_temp_mut_or_default::<NativeCursors>(native_cursors_id())
                .0
                .clear();
        });
        crate::kit::img::forget_all(ctx);
    }
    fn user_file(ctx: &Context, kind: &str, name: &str) -> Option<std::sync::Arc<[u8]>> {
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<UserPack>(egui::Id::unique("paintrs_user_pack"))
                .0
                .get(&format!("assets/{kind}/{name}"))
                .cloned()
        })
    }
    fn user_render_file(ctx: &Context, name: &str) -> Option<std::sync::Arc<[u8]>> {
        let kind = if UI_IMAGES.contains(&name) {
            "images"
        } else {
            "icons"
        };
        Self::user_file(ctx, kind, name)
    }
    pub fn icon_source(ctx: &Context, name: &str) -> IconSource {
        if ctx.data(|d| {
            d.get_temp::<Overrides>(egui::Id::unique("paintrs_icon_overrides"))
                .is_some_and(|o| o.0.contains_key(name))
        }) {
            return IconSource::UserPack;
        }
        if matches!(Self::selected(ctx), crate::icon_pack::IconPackId::User(_))
            && Self::user_render_file(ctx, name).is_some()
        {
            return IconSource::UserPack;
        }
        IconSource::PaintRs
    }

    /// Replaces an icon with icon-pack bytes (`AssetPackManager.resolve`).
    pub fn set_icon_override(ctx: &Context, name: &str, png: Vec<u8>) {
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Overrides>(egui::Id::unique("paintrs_icon_overrides"))
                .0
                .insert(name.into(), png);
            let icon_prefix = format!("icons/{name}/");
            let image_prefix = format!("images/{name}/");
            d.get_temp_mut_or_default::<Cache>(egui::Id::unique("paintrs_textures"))
                .retain(|key, _| !key.starts_with(&icon_prefix) && !key.starts_with(&image_prefix));
        });
        // JS AssetPackManager.activate -> applyToTree swaps live <img> sources.
        crate::kit::img::forget_icon(ctx, name);
    }

    /// An icon pack replaced `name` (its `<img>` is no longer `data-asset-pack-source="built-in"`).
    pub fn is_overridden(ctx: &Context, name: &str) -> bool {
        (matches!(Self::selected(ctx), crate::icon_pack::IconPackId::User(_))
            && Self::user_render_file(ctx, name).is_some())
            || ctx.data_mut(|d| {
                d.get_temp_mut_or_default::<Overrides>(egui::Id::unique("paintrs_icon_overrides"))
                    .0
                    .contains_key(name)
            })
    }

    fn texture(
        ctx: &Context,
        key: String,
        make: impl FnOnce() -> Option<ColorImage>,
    ) -> Option<TextureHandle> {
        let id = egui::Id::unique("paintrs_textures");
        if let Some(t) = ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(id).get(&key).cloned())
        {
            return Some(t);
        }
        let tex = ctx.load_texture(&key, make()?, TextureOptions::NEAREST);
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Cache>(id)
                .insert(key, tex.clone())
        });
        Some(tex)
    }

    /// `name` is the file name, e.g. `menu_file_new_icon.png`.
    pub fn icon(ctx: &Context, name: &str) -> Option<TextureHandle> {
        let key = format!(
            "icons/{name}/{:?}/{}/{}",
            Self::selected(ctx),
            ctx.pixels_per_point().to_bits(),
            dark_theme(ctx)
        );
        Self::texture(ctx, key, || {
            Some(color_image(&Self::icon_image(ctx, name)?))
        })
    }

    /// Decoded straight-RGBA icon pixels, honoring the chosen pack and fallback chain.
    pub fn icon_image(ctx: &Context, name: &str) -> Option<image::RgbaImage> {
        let override_bytes = ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Overrides>(egui::Id::unique("paintrs_icon_overrides"))
                .0
                .get(name)
                .cloned()
        });
        if let Some(bytes) = override_bytes {
            return image::load_from_memory(&bytes).ok().map(|i| i.to_rgba8());
        }
        match Self::icon_source(ctx, name) {
            IconSource::UserPack => Self::user_render_file(ctx, name)
                .and_then(|bytes| image::load_from_memory(&bytes).ok().map(|i| i.to_rgba8())),
            IconSource::PaintRs => Self::svg_image(ctx, name),
        }
    }

    pub fn image(ctx: &Context, name: &str) -> Option<TextureHandle> {
        let key = format!(
            "images/{name}/{:?}/{}/{}",
            Self::selected(ctx),
            ctx.pixels_per_point().to_bits(),
            dark_theme(ctx)
        );
        Self::texture(ctx, key, || {
            Some(color_image(&Self::icon_image(ctx, name)?))
        })
    }

    /// `name` without extension, e.g. `paint_brush_tool_cursor`: a user pack's PNG, else the
    /// paint.rs SVG.
    pub fn cursor(ctx: &Context, name: &str) -> Option<Cursor> {
        use crate::icon_pack::IconPackId;
        let selected = Self::selected(ctx);
        let user = if matches!(selected, IconPackId::User(_)) {
            Self::user_file(ctx, "cursors", &format!("{name}.png"))
                .or_else(|| Self::user_file(ctx, "cursors", &format!("{name}.webp")))
        } else {
            None
        };
        let png = user.as_deref().and_then(try_decode_cursor);
        let ppp = ctx.pixels_per_point();
        let (hotspot, scale) = match &png {
            Some((_, hotspot)) => (*hotspot, 1.0),
            None => {
                let [x, y] = cursor_hotspot(name)?;
                ([(x * ppp).round() as u32, (y * ppp).round() as u32], ppp)
            }
        };
        let texture = Self::texture(
            ctx,
            format!(
                "cursors/{name}/{selected:?}/{}/{}",
                ppp.to_bits(),
                dark_theme(ctx)
            ),
            || match png {
                Some((img, _)) => Some(img),
                None => Some(color_image(&raster_svg(
                    lookup(embedded::ICON_SVGS, &format!("{name}.svg"))?,
                    ppp,
                )?)),
            },
        )?;
        Some(Cursor {
            texture,
            hotspot,
            scale,
        })
    }

    /// [`Assets::cursor`] as an OS cursor bitmap (straight RGBA, hotspot in its pixels: device
    /// pixels, or points on macOS, see [`CURSOR_IN_POINTS`]), cached per pack, scale and theme:
    /// the compositor moves it with the pointer, so it never trails the frame rate as a painted
    /// cursor does. A PNG cursor (one image pixel per point) is nearest-scaled to device pixels,
    /// as its texture is drawn.
    pub fn native_cursor(ctx: &Context, name: &str) -> Option<egui::CustomCursorImage> {
        let ppp = ctx.pixels_per_point();
        let key = format!(
            "{name}/{:?}/{}/{}",
            Self::selected(ctx),
            ppp.to_bits(),
            dark_theme(ctx)
        );
        if let Some(hit) = ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<NativeCursors>(native_cursors_id())
                .0
                .get(&key)
                .cloned()
        }) {
            return hit;
        }
        let made = Self::make_native_cursor(ctx, name, ppp, CURSOR_IN_POINTS);
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<NativeCursors>(native_cursors_id())
                .0
                .insert(key, made.clone())
        });
        made
    }

    /// The bitmap at `ppp` device pixels per point, or at one pixel per point when `points`.
    fn make_native_cursor(
        ctx: &Context,
        name: &str,
        ppp: f32,
        points: bool,
    ) -> Option<egui::CustomCursorImage> {
        use crate::icon_pack::IconPackId;
        let ppp = if points { 1.0 } else { ppp };
        let selected = Self::selected(ctx);
        let user = if matches!(selected, IconPackId::User(_)) {
            Self::user_file(ctx, "cursors", &format!("{name}.png"))
                .or_else(|| Self::user_file(ctx, "cursors", &format!("{name}.webp")))
        } else {
            None
        };
        let png = user.as_deref().and_then(decode_cursor_rgba);
        let device = |v: u32| (v as f32 * ppp).round() as u32;
        let (img, [hx, hy]) = match png {
            Some((img, [hx, hy])) => {
                let (w, h) = img.dimensions();
                // The texel under each device pixel's centre.
                let at = |v: u32| ((v as f32 + 0.5) / ppp) as u32;
                let scaled =
                    image::RgbaImage::from_fn(device(w).max(1), device(h).max(1), |x, y| {
                        *img.get_pixel(at(x).min(w - 1), at(y).min(h - 1))
                    });
                (scaled, [device(hx), device(hy)])
            }
            None => {
                let [x, y] = cursor_hotspot(name)?;
                let img = raster_svg(lookup(embedded::ICON_SVGS, &format!("{name}.svg"))?, ppp)?;
                let hotspot = [(x * ppp).round() as u32, (y * ppp).round() as u32];
                (img, hotspot)
            }
        };
        let (w, h) = img.dimensions();
        Some(egui::CustomCursorImage {
            rgba: img.into_raw().into(),
            size: [u16::try_from(w).ok()?, u16::try_from(h).ok()?],
            hotspot: [hx.min(w - 1) as u16, hy.min(h - 1) as u16],
        })
    }
}

/// `ActionItem.getIconPath` (with `iconPathKey` = "icon"): `.` -> `_`, `_` before each capital,
/// lowercase, then `_icon.png`.
pub fn action_icon_name(action_id: &str) -> String {
    let mut s = String::new();
    for c in action_id.chars() {
        match c {
            '.' => s.push('_'),
            c if c.is_ascii_uppercase() => {
                s.push('_');
                s.push(c.to_ascii_lowercase());
            }
            c => s.extend(c.to_lowercase()),
        }
    }
    s + "_icon.png"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_names() {
        assert_eq!(action_icon_name("menu.file.new"), "menu_file_new_icon.png");
        assert_eq!(
            action_icon_name("menu.layers.addNewLayer"),
            "menu_layers_add_new_layer_icon.png"
        );
    }

    #[test]
    fn every_bundled_svg_rasterizes() {
        assert_eq!(embedded::ICON_SVGS.len(), 478);
        for (name, bytes) in embedded::ICON_SVGS {
            assert!(raster_svg(bytes, 1.0).is_some(), "{name}");
        }
    }

    #[test]
    fn live_override_invalidates_the_rendered_texture() {
        let ctx = Context::default();
        let name = "menu_file_new_icon.png";
        let before = Assets::icon(&ctx, name).unwrap().id();
        let mut png = vec![];
        app_icon(16)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        Assets::set_icon_override(&ctx, name, png);
        assert_ne!(Assets::icon(&ctx, name).unwrap().id(), before);
    }

    #[test]
    fn app_icon_rasterizes_at_16_and_256() {
        for side in [16, 256] {
            let icon = app_icon(side);
            assert_eq!(icon.dimensions(), (side, side));
            assert!(icon.pixels().any(|p| p.0[3] == 255), "{side}");
        }
    }

    /// The 454 × 329 logo keeps its aspect: transparent bands above and below it.
    #[test]
    fn app_icon_is_centred_undistorted() {
        let icon = app_icon(256);
        let band = |rows: std::ops::Range<u32>| rows.flat_map(|y| (0..256).map(move |x| (x, y)));
        for (x, y) in band(0..26).chain(band(230..256)) {
            assert_eq!(icon.get_pixel(x, y).0[3], 0, "({x}, {y})");
        }
    }

    /// Fails unless `stored` is `fresh` within 2 per premultiplied channel: tiny-skia's SIMD
    /// pipelines may differ by an LSB across CPUs, which straight colour magnifies by 255 / alpha
    /// at faint edges; an SVG edit moves pixels far more.
    fn assert_matches_render(stored: &image::RgbaImage, fresh: &image::RgbaImage, what: &str) {
        assert_eq!(stored.dimensions(), fresh.dimensions(), "{what}");
        let premul = |p: &image::Rgba<u8>, c: usize| match c {
            3 => p.0[3] as u32,
            c => (p.0[c] as u32 * p.0[3] as u32 + 127) / 255,
        };
        let diff = |a: &image::Rgba<u8>, b: &image::Rgba<u8>| {
            (0..4)
                .map(|c| premul(a, c).abs_diff(premul(b, c)))
                .max()
                .unwrap()
        };
        let (d, x, y) = stored
            .enumerate_pixels()
            .map(|(x, y, p)| (diff(p, fresh.get_pixel(x, y)), x, y))
            .max()
            .unwrap();
        assert!(
            d <= 2,
            "stale {what}: ({x}, {y}) is {:?}, a fresh render {:?} (premultiplied off by {d}); \
             PAINTRS_WRITE_APP_ICON=1 rewrites it",
            stored.get_pixel(x, y).0,
            fresh.get_pixel(x, y).0
        );
    }

    /// `assets/app-icon.png` (the macOS `.icns` source) is `app-icon.svg` rendered 1024 px.
    /// `PAINTRS_WRITE_APP_ICON=1` rewrites it (and `app-icon.ico`) after an SVG change.
    #[test]
    fn app_icon_png_matches_the_svg() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/app-icon.png");
        if std::env::var_os("PAINTRS_WRITE_APP_ICON").is_some() {
            app_icon(1024).save(path).unwrap();
        }
        let png = image::open(path).unwrap().into_rgba8();
        assert_matches_render(&png, &app_icon(1024), path);
    }

    /// The Windows .exe icon's sizes (build.rs embeds `assets/app-icon.ico`).
    const ICO_SIDES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];

    /// An .ico of `images`: 32-bit DIBs (BGRA rows bottom up, then the 1-bit transparency mask),
    /// as every Windows version reads them, and a PNG for 256 px, as Windows Vista and later do.
    fn ico(images: &[image::RgbaImage]) -> Vec<u8> {
        let mut head = [0u16, 1, images.len() as u16]
            .map(u16::to_le_bytes)
            .concat();
        let mut body = vec![];
        for image in images {
            let (w, h) = image.dimensions();
            let mut data = vec![];
            if w >= 256 {
                image
                    .write_to(
                        &mut std::io::Cursor::new(&mut data),
                        image::ImageFormat::Png,
                    )
                    .unwrap();
            } else {
                let mask_row = w.div_ceil(32) as usize * 4;
                let size = w * h * 4 + mask_row as u32 * h;
                for v in [40, w, 2 * h, 1 | 32 << 16, 0, size, 0, 0, 0, 0] {
                    data.extend(u32::to_le_bytes(v));
                }
                for y in (0..h).rev() {
                    for x in 0..w {
                        let [r, g, b, a] = image.get_pixel(x, y).0;
                        data.extend([b, g, r, a]);
                    }
                }
                for y in (0..h).rev() {
                    let mut row = vec![0u8; mask_row];
                    for x in (0..w).filter(|&x| image.get_pixel(x, y).0[3] == 0) {
                        row[x as usize / 8] |= 0x80 >> (x % 8);
                    }
                    data.extend(row);
                }
            }
            let offset = 6 + 16 * images.len() + body.len();
            head.extend([w as u8, h as u8, 0, 0, 1, 0, 32, 0]);
            head.extend((data.len() as u32).to_le_bytes());
            head.extend((offset as u32).to_le_bytes());
            body.extend(data);
        }
        head.extend(body);
        head
    }

    /// `assets/app-icon.ico` holds `app-icon.svg` rendered at each of `ICO_SIDES`.
    #[test]
    fn app_icon_ico_matches_the_svg() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/app-icon.ico");
        if std::env::var_os("PAINTRS_WRITE_APP_ICON").is_some() {
            std::fs::write(path, ico(&ICO_SIDES.map(app_icon))).unwrap();
        }
        let bytes = std::fs::read(path).unwrap();
        let images = crate::platform::winres::ico_images(&bytes).unwrap();
        assert_eq!(images.len(), ICO_SIDES.len(), "{path}");
        for (image, side) in images.iter().zip(ICO_SIDES) {
            assert_eq!(image.entry[..2], [side as u8; 2], "{side} px entry");
            let stored = if side == 256 {
                image::load_from_memory_with_format(image.data, image::ImageFormat::Png)
                    .unwrap()
                    .into_rgba8()
            } else {
                let le = |at: usize| u32::from_le_bytes(image.data[at..at + 4].try_into().unwrap());
                assert_eq!(
                    [le(0), le(4), le(8), le(12)],
                    [40, side, 2 * side, 1 | 32 << 16]
                );
                image::RgbaImage::from_fn(side, side, |x, y| {
                    let at = 40 + 4 * ((side - 1 - y) * side + x) as usize;
                    let [b, g, r, a] = image.data[at..at + 4].try_into().unwrap();
                    image::Rgba([r, g, b, a])
                })
            };
            assert_matches_render(&stored, &app_icon(side), &format!("{path} at {side} px"));
        }
    }

    /// R213: the title-bar icon and About banner are the app icon under every pack, also when it
    /// changes at runtime.
    #[test]
    fn app_artwork_follows_the_pack() {
        use crate::icon_pack::IconPackId;
        let ctx = Context::default();
        for pack in [IconPackId::PaintRs, IconPackId::User("u".into())] {
            Assets::select(&ctx, pack.clone());
            assert!(*Assets::app_artwork(&ctx, 20) == app_icon(20), "{pack:?}");
        }
    }

    #[test]
    fn embedded_tables_sorted_and_found() {
        assert!(lookup(embedded::ICON_SVGS, "menu_file_new_icon.svg").is_some());
        assert!(lookup(embedded::ICON_SVGS, "hand_open_cursor.svg").is_some());
    }

    /// pdn-core's reset button icon: the paint.rs SVG at 32 px.
    #[test]
    fn reset_nub_icon_is_the_packs_artwork() {
        let icon = reset_nub_icon().unwrap();
        assert_eq!((icon.width(), icon.height()), (32, 32));
        assert!(icon.pixels().iter().any(|p| p.alpha() == 255));
    }

    /// Every tool cursor is a paint.rs SVG with a manifest hotspot, rasterized per device scale
    /// (crisp: one texture pixel per device pixel) and drawn 32 points square.
    #[test]
    fn svg_cursors_follow_the_device_scale() {
        let manifest: HashMap<String, [f32; 2]> = serde_json::from_str(include_str!(
            "../../../assets/icons-svg/cursors/manifest.json"
        ))
        .unwrap();
        let svgs = embedded::ICON_SVGS
            .iter()
            .filter(|(n, _)| n.ends_with("_cursor.svg"))
            .count();
        assert_eq!((manifest.len(), svgs), (25, 25));
        let ctx = Context::default();
        Assets::select(&ctx, crate::icon_pack::IconPackId::PaintRs);
        for (ppp, side) in [(1.0, 32), (1.25, 40), (2.0, 64)] {
            crate::seed_native_scale(&ctx, ppp);
            for (name, [x, y]) in &manifest {
                let c = Assets::cursor(&ctx, name).unwrap_or_else(|| panic!("{name}"));
                assert_eq!(c.texture.size(), [side, side], "{name} @ {ppp}");
                assert_eq!(c.scale, ppp);
                let want = [(x * ppp).round() as u32, (y * ppp).round() as u32];
                assert_eq!(c.hotspot, want, "{name} @ {ppp}");
            }
        }
    }

    /// An OS cursor bitmap per platform branch: device pixels (Windows, X11, Wayland, the web) or
    /// one pixel per point (macOS), the hotspot rounded (17.6 to 18 at 1.1); straight RGBA, as
    /// `raster_svg` gives it.
    #[test]
    fn native_cursor_bitmaps_per_platform() {
        let ctx = Context::default();
        Assets::select(&ctx, crate::icon_pack::IconPackId::PaintRs);
        let name = "paint_brush_tool_cursor";
        let svg = lookup(embedded::ICON_SVGS, &format!("{name}.svg")).unwrap();
        for (ppp, points, side, hot, raster) in [
            (1.0, false, 32, 16, 1.0),
            (1.1, false, 35, 18, 1.1),
            (1.25, false, 40, 20, 1.25),
            (2.0, false, 64, 32, 2.0),
            (1.25, true, 32, 16, 1.0),
            (2.0, true, 32, 16, 1.0),
        ] {
            let c = Assets::make_native_cursor(&ctx, name, ppp, points).unwrap();
            assert_eq!((c.size, c.hotspot), ([side; 2], [hot; 2]), "{ppp} {points}");
            assert_eq!(
                *c.rgba,
                *raster_svg(svg, raster).unwrap().into_raw(),
                "{ppp}"
            );
        }
    }

    /// A user pack's PNG cursor (one image pixel per point, hotspot in its metadata row and
    /// column): nearest-scaled at device pixel centres, or kept as is per point.
    #[test]
    fn native_png_cursors_scale_to_device_pixels() {
        use crate::icon_pack::IconPackId;
        // 4 x 4 pixels and the metadata column and row: the hotspot is (1, 2).
        let red = image::Rgba([255, 0, 0, 255]);
        let png = image::RgbaImage::from_fn(5, 5, |x, y| match (x, y) {
            (4, 2) | (1, 4) => red,
            (4, _) | (_, 4) => image::Rgba([0; 4]),
            (x, y) => image::Rgba([x as u8, y as u8, 7, 255]),
        });
        let mut bytes = vec![];
        png.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
        let ctx = Context::default();
        let name = "paint_brush_tool_cursor";
        let files = [(format!("assets/cursors/{name}.png"), bytes)].into();
        Assets::set_pack(
            &ctx,
            Some(pdn_core::asset_pack::AssetPack {
                id: "u".into(),
                manifest: serde_json::json!({}),
                files,
                translations: Default::default(),
            }),
        );
        Assets::select(&ctx, IconPackId::User("u".into()));
        // At 1.5 the device columns' centres fall on texels 0, 1, 1, 2, 3, 3.
        for (points, side, hot, at) in [
            (false, 6, [2, 3], &[0, 1, 1, 2, 3, 3][..]),
            (true, 4, [1, 2], &[0, 1, 2, 3][..]),
        ] {
            let c = Assets::make_native_cursor(&ctx, name, 1.5, points).unwrap();
            assert_eq!((c.size, c.hotspot), ([side; 2], hot), "{points}");
            let want: Vec<u8> = (at.iter())
                .flat_map(|&y| at.iter().flat_map(move |&x| [x, y, 7, 255]))
                .collect();
            assert_eq!(*c.rgba, *want, "{points}");
        }
    }
}

#[cfg(test)]
mod pack_tests {
    use super::*;

    #[test]
    fn selection_changes_source_and_invalidates_texture() {
        let ctx = Context::default();
        let name = "menu_file_new_icon.png";
        Assets::select(&ctx, crate::icon_pack::IconPackId::PaintRs);
        let paint_rs = Assets::icon_image(&ctx, name).unwrap();
        let first = Assets::icon(&ctx, name).unwrap().id();
        assert_eq!(Assets::icon_source(&ctx, name), IconSource::PaintRs);
        Assets::select(&ctx, crate::icon_pack::IconPackId::User("missing".into()));
        // A pack without the icon draws paint.rs's, from a texture of its own.
        assert_eq!(Assets::icon_source(&ctx, name), IconSource::PaintRs);
        assert_eq!(Assets::icon_image(&ctx, name).unwrap(), paint_rs);
        assert_ne!(Assets::icon(&ctx, name).unwrap().id(), first);
        assert_eq!(
            Assets::icon_source(&ctx, "settings_assets_24.png"),
            IconSource::PaintRs
        );
    }

    #[test]
    fn svg_raster_is_deterministic_at_fractional_scales() {
        let ctx = Context::default();
        Assets::select(&ctx, crate::icon_pack::IconPackId::PaintRs);
        for (ppp, pixels) in [(1.0, 16), (1.25, 20), (2.0, 32)] {
            crate::seed_native_scale(&ctx, ppp);
            let image = Assets::icon_image(&ctx, "menu_file_new_icon.png").unwrap();
            assert_eq!(image.dimensions(), (pixels, pixels));
            assert_eq!(
                image,
                Assets::icon_image(&ctx, "menu_file_new_icon.png").unwrap()
            );
        }
    }
}
