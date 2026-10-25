//! Device-pixel images uploaded as textures no larger than the renderer's `max_texture_side`:
//! `Context::load_texture` panics past it (a 2560-DIP canvas at 2× is over 5000 px wide, a
//! near-full-height form's shadow at 2560×1440 @2× over 2900 px tall). The canvas view, the
//! rulers and the shadow/veil textures share it.
use std::collections::HashMap;
use std::sync::Arc;

use egui::{
    Color32, ColorImage, Context, Id, ImageData, Painter, Pos2, Rect, TextureHandle, TextureId,
};
use egui::{TextureOptions, Vec2};
use pdn_core::pixel::IntRect;

/// The upload log of every tile texture, by id. Tile textures change pixels only through
/// [`Tiles::upload`], which logs each upload under a new version (and egui never reuses an id),
/// so `(id, version)` names a tile's pixels and the log says where they changed since (the
/// shadow cache keys on both).
#[derive(Clone, Default)]
struct Versions {
    last: u64,
    by_id: HashMap<TextureId, Log>,
}

/// A texture's latest version and its recent uploads: `(version, texel rect)`, `None` for all of
/// it, oldest first; versions up to `forgotten` fell out of it.
#[derive(Clone, Default)]
struct Log {
    version: u64,
    forgotten: u64,
    uploads: std::collections::VecDeque<(u64, Option<IntRect>)>,
}

/// Uploads kept per texture: an ants-only frame uploads about two rects per 32-row band of the
/// canvas view, about 270 for a 4320-px-tall one, and a key older than the log misses.
const LOGGED: usize = 512;

fn versions_id() -> Id {
    Id::unique("paintrs.kit.tiles.versions")
}

/// Each id's version: 0 for a texture [`Tiles`] never uploaded.
pub(crate) fn versions(ctx: &Context, ids: &[TextureId]) -> Vec<u64> {
    if ids.is_empty() {
        return vec![];
    }
    ctx.data_mut(|d| {
        let v = d.get_temp_mut_or_default::<Versions>(versions_id());
        ids.iter()
            .map(|id| v.by_id.get(id).map_or(0, |l| l.version))
            .collect()
    })
}

/// Whether texture `id` changed inside texel rect `region` after version `since` (true when the
/// log no longer reaches back that far).
pub(crate) fn changed_within(ctx: &Context, id: TextureId, since: u64, region: IntRect) -> bool {
    ctx.data_mut(|d| {
        let v = d.get_temp_mut_or_default::<Versions>(versions_id());
        let Some(log) = v.by_id.get(&id) else {
            return since != 0;
        };
        if log.version == since {
            return false;
        }
        since < log.forgotten
            || log.uploads.iter().any(|&(version, rect)| {
                version > since && rect.is_none_or(|r| !r.intersect(&region).is_empty())
            })
    })
}

/// Logs an upload of `rect` (`None`: all) to each texture.
fn bump(ctx: &Context, ids: impl IntoIterator<Item = TextureId>, rect: Option<IntRect>) {
    ctx.data_mut(|d| {
        let v = d.get_temp_mut_or_default::<Versions>(versions_id());
        for id in ids {
            v.last += 1;
            let log = v.by_id.entry(id).or_default();
            log.version = v.last;
            log.uploads.push_back((v.last, rect));
            if log.uploads.len() > LOGGED
                && let Some((dropped, _)) = log.uploads.pop_front()
            {
                log.forgotten = dropped;
            }
        }
    });
}

/// Drops the versions of freed textures.
fn prune(ctx: &Context) {
    let ids: Vec<TextureId> = ctx.data_mut(|d| {
        let v = d.get_temp_mut_or_default::<Versions>(versions_id());
        v.by_id.keys().copied().collect()
    });
    let manager = ctx.tex_manager();
    let dead: Vec<TextureId> = {
        let manager = manager.read();
        ids.into_iter()
            .filter(|id| manager.meta(*id).is_none())
            .collect()
    };
    ctx.data_mut(|d| {
        let v = d.get_temp_mut_or_default::<Versions>(versions_id());
        for id in dead {
            v.by_id.remove(&id);
        }
    });
}

/// The `r` sub-image of the `w`-wide `pixels`.
pub(crate) fn crop(pixels: &[Color32], w: usize, r: IntRect) -> ColorImage {
    let (x0, rw) = (r.x as usize, r.w as usize);
    let mut out = Vec::with_capacity(rw * r.h as usize);
    for y in r.y as usize..r.bottom() as usize {
        out.extend_from_slice(&pixels[y * w + x0..][..rw]);
    }
    ColorImage::new([rw, r.h as usize], out)
}

/// An image's textures, one per `max_texture_side` square. One tile, the usual case, shares the
/// image's `Arc` instead of a copy.
#[derive(Clone, Default)]
pub(crate) struct Tiles(Vec<(TextureHandle, IntRect)>);

impl Tiles {
    /// The `side`-square grid over `size`, row by row.
    pub(crate) fn grid(size: [usize; 2], side: usize) -> Vec<IntRect> {
        let side = side.max(1);
        let cells = |n: usize| {
            (0..n.max(1))
                .step_by(side)
                .map(move |a| (a, side.min(n - a)))
        };
        cells(size[1])
            .flat_map(|(y, h)| {
                cells(size[0])
                    .map(move |(x, w)| IntRect::new(x as i32, y as i32, w as u32, h as u32))
            })
            .collect()
    }

    /// Where a `dirty` rect of the image lands: per touched tile of `grid`, its index, the offset
    /// inside the tile and the image rect to copy.
    pub(crate) fn parts(grid: &[IntRect], dirty: IntRect) -> Vec<(usize, [usize; 2], IntRect)> {
        grid.iter()
            .enumerate()
            .filter_map(|(k, r)| {
                let i = dirty.intersect(r);
                (!i.is_empty()).then(|| (k, [(i.x - r.x) as usize, (i.y - r.y) as usize], i))
            })
            .collect()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn ids(&self) -> Vec<TextureId> {
        self.0.iter().map(|(t, _)| t.id()).collect()
    }

    fn tile(image: &Arc<ColorImage>, r: IntRect, single: bool) -> ImageData {
        if single {
            ImageData::Color(image.clone())
        } else {
            ImageData::Color(Arc::new(crop(&image.pixels, image.size[0], r)))
        }
    }

    /// Uploads `image`: whole when `dirty` is `None`, else only the `dirty` device rect.
    pub(crate) fn upload(
        &mut self,
        ctx: &Context,
        name: &str,
        image: &Arc<ColorImage>,
        dirty: Option<IntRect>,
    ) {
        let grid = Self::grid(image.size, ctx.input(|i| i.max_texture_side));
        let single = grid.len() == 1;
        if !self.0.iter().map(|(_, r)| *r).eq(grid.iter().copied()) {
            self.0 = grid
                .into_iter()
                .map(|r| {
                    let tile = Self::tile(image, r, single);
                    (ctx.load_texture(name, tile, TextureOptions::NEAREST), r)
                })
                .collect();
            // The replaced tiles' textures are freed unless a clone still holds them.
            prune(ctx);
            bump(ctx, self.0.iter().map(|(t, _)| t.id()), None);
            return;
        }
        // A whole tile goes in as an update from its corner: egui's renderers make a new GPU
        // texture (and bind group) for a full `set`, which a pan or zoom did every frame.
        match dirty {
            None => {
                for (tex, r) in &mut self.0 {
                    tex.set_partial(
                        [0, 0],
                        Self::tile(image, *r, single),
                        TextureOptions::NEAREST,
                    );
                }
                bump(ctx, self.0.iter().map(|(t, _)| t.id()), None);
            }
            // Most of a lone tile: its pixels as they are (no copy), logged as the rect alone
            // (the rest uploads unchanged).
            Some(d)
                if single && 2 * d.w as usize * d.h as usize > image.size[0] * image.size[1] =>
            {
                let (tex, _) = &mut self.0[0];
                tex.set_partial(
                    [0, 0],
                    Self::tile(image, grid[0], true),
                    TextureOptions::NEAREST,
                );
                bump(ctx, [tex.id()], Some(d.intersect(&grid[0])));
            }
            Some(d) => {
                for (k, at, i) in Self::parts(&grid, d) {
                    let part = crop(&image.pixels, image.size[0], i);
                    self.0[k].0.set_partial(at, part, TextureOptions::NEAREST);
                    let texels = IntRect::new(at[0] as i32, at[1] as i32, i.w, i.h);
                    bump(ctx, [self.0[k].0.id()], Some(texels));
                }
            }
        }
    }

    /// Whether the image is in one texture.
    pub(crate) fn single(&self) -> bool {
        self.0.len() == 1
    }

    /// Copies rect `src` of `image` into the lone tile at texel `at`, and logs those texels.
    pub(crate) fn upload_at(
        &mut self,
        ctx: &Context,
        image: &Arc<ColorImage>,
        src: IntRect,
        at: [usize; 2],
    ) {
        let part = crop(&image.pixels, image.size[0], src);
        let (tex, _) = &mut self.0[0];
        tex.set_partial(at, part, TextureOptions::NEAREST);
        let texels = IntRect::new(at[0] as i32, at[1] as i32, src.w, src.h);
        bump(ctx, [tex.id()], Some(texels));
    }

    /// The lone tile, one texel per device pixel from the device-aligned `min`, with device px
    /// (x, y) taking texel ((x + wrap[0]) mod w, (y + wrap[1]) mod h): up to four quads.
    pub(crate) fn paint_wrapped(&self, painter: &Painter, min: Pos2, ppp: f32, wrap: [u32; 2]) {
        let (tex, r) = &self.0[0];
        let (w, h) = (r.w, r.h);
        // (first device px, first texel, length) of each piece.
        let pieces = |off: u32, n: u32| [(0, off, n - off), (n - off, 0, off)];
        for (x, tx, cw) in pieces(wrap[0], w).into_iter().filter(|p| p.2 > 0) {
            for (y, ty, ch) in pieces(wrap[1], h).into_iter().filter(|p| p.2 > 0) {
                let uv = |t: u32, n: u32| t as f32 / n as f32;
                painter.image(
                    tex.id(),
                    Rect::from_min_size(
                        min + Vec2::new(x as f32, y as f32) / ppp,
                        Vec2::new(cw as f32, ch as f32) / ppp,
                    ),
                    Rect::from_min_max(
                        Pos2::new(uv(tx, w), uv(ty, h)),
                        Pos2::new(uv(tx + cw, w), uv(ty + ch, h)),
                    ),
                    Color32::WHITE,
                );
            }
        }
    }

    /// One texel per device pixel from the device-aligned `min`.
    pub(crate) fn paint(&self, painter: &Painter, min: Pos2, ppp: f32) {
        self.paint_scaled(painter, min, ppp, 1.0);
    }

    /// One texel per `scale` × `scale` device pixels from the device-aligned `min`.
    pub(crate) fn paint_scaled(&self, painter: &Painter, min: Pos2, ppp: f32, scale: f32) {
        for (tex, r) in &self.0 {
            painter.image(
                tex.id(),
                Rect::from_min_size(
                    min + Vec2::new(r.x as f32, r.y as f32) * scale / ppp,
                    Vec2::new(r.w as f32, r.h as f32) * scale / ppp,
                ),
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    /// The tiles' ids with their pixels, cut from `image`.
    pub(crate) fn each_pixels<'a>(
        &'a self,
        image: &'a Arc<ColorImage>,
    ) -> impl Iterator<Item = (TextureId, Arc<ColorImage>)> + 'a {
        self.0
            .iter()
            .filter_map(|(t, _)| Some((t.id(), self.pixels(t.id(), image)?)))
    }

    /// The pixels of tile `id` of `image`.
    // ponytail: a multi-tile image crops a fresh copy per lookup (only the backdrop replay under
    // shadows and veils asks, and only past `max_texture_side`); keep the crops if that shows up.
    pub(crate) fn pixels(&self, id: TextureId, image: &Arc<ColorImage>) -> Option<Arc<ColorImage>> {
        let (_, r) = self.0.iter().find(|(t, _)| t.id() == id)?;
        Some(match self.0.len() {
            1 => image.clone(),
            _ => Arc::new(crop(&image.pixels, image.size[0], *r)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i32, y: i32, w: u32, h: u32) -> IntRect {
        IntRect::new(x, y, w, h)
    }

    /// Every upload is logged where it changed the texture: the shadow cache keeps an image
    /// whose quads read none of it.
    #[test]
    fn uploads_log_where_the_texels_changed() {
        let ctx = Context::default();
        let image = Arc::new(ColorImage::filled([64, 32], Color32::WHITE));
        let mut tiles = Tiles::default();
        tiles.upload(&ctx, "t", &image, None);
        let id = tiles.0[0].0.id();
        let version = |ctx: &Context| versions(ctx, &[id])[0];
        let v0 = version(&ctx);
        assert!(v0 > 0);
        tiles.upload(&ctx, "t", &image, Some(r(10, 4, 5, 5)));
        assert!(version(&ctx) > v0);
        assert!(changed_within(&ctx, id, v0, r(12, 6, 1, 1)));
        assert!(!changed_within(&ctx, id, v0, r(40, 4, 8, 8)));
        assert!(!changed_within(&ctx, id, version(&ctx), r(12, 6, 1, 1)));
        // A whole upload changes every texel.
        let v1 = version(&ctx);
        tiles.upload(&ctx, "t", &image, None);
        assert!(changed_within(&ctx, id, v1, r(40, 4, 8, 8)));
        // Past the log's reach: changed, whatever the rect.
        let v2 = version(&ctx);
        for _ in 0..=LOGGED {
            tiles.upload(&ctx, "t", &image, Some(r(0, 0, 1, 1)));
        }
        assert!(changed_within(&ctx, id, v2, r(40, 4, 8, 8)));
        // A texture `Tiles` never uploaded keeps version 0.
        assert_eq!(versions(&ctx, &[TextureId::User(7)]), [0]);
        assert!(!changed_within(&ctx, TextureId::User(7), 0, r(0, 0, 1, 1)));
        // Past `max_texture_side` each tile logs its own texels, from the tile's corner.
        let ctx = Context::default();
        ctx.input_mut(|i| i.max_texture_side = 16);
        let mut tiles = Tiles::default();
        tiles.upload(&ctx, "t", &image, None);
        let ids: Vec<TextureId> = tiles.0.iter().map(|(t, _)| t.id()).collect();
        assert_eq!(ids.len(), 8);
        let v0 = versions(&ctx, &ids);
        tiles.upload(&ctx, "t", &image, Some(r(20, 4, 2, 2)));
        assert!(changed_within(&ctx, ids[1], v0[1], r(4, 4, 1, 1)));
        assert!(!changed_within(&ctx, ids[1], v0[1], r(8, 8, 8, 8)));
        assert!(!changed_within(&ctx, ids[0], v0[0], r(0, 0, 16, 16)));
    }

    /// Whole uploads of an existing texture update it in place (a full delta would make the
    /// renderer allocate a new GPU texture).
    #[test]
    fn whole_uploads_update_the_texture_in_place() {
        let ctx = Context::default();
        let image = Arc::new(ColorImage::filled([64, 32], Color32::WHITE));
        let mut tiles = Tiles::default();
        let positions = |ctx: &Context, tiles: &Tiles| -> Vec<Option<[usize; 2]>> {
            let mut delta = ctx.tex_manager().write().take_delta();
            let positions = delta.set[&tiles.ids()[0]].iter().map(|d| d.pos).collect();
            delta.clear();
            positions
        };
        tiles.upload(&ctx, "t", &image, None);
        assert_eq!(positions(&ctx, &tiles), [None]);
        tiles.upload(&ctx, "t", &image, None);
        tiles.upload(&ctx, "t", &image, Some(r(0, 0, 60, 30)));
        assert_eq!(positions(&ctx, &tiles), [Some([0, 0]), Some([0, 0])]);
    }

    /// Textures stay within `max_texture_side` (kittest's and egui's default is 2048).
    #[test]
    fn tiles_cover_the_image_within_the_side() {
        assert_eq!(Tiles::grid([1280, 672], 2048), [r(0, 0, 1280, 672)]);
        assert_eq!(
            Tiles::grid([5000, 2100], 2048),
            [
                r(0, 0, 2048, 2048),
                r(2048, 0, 2048, 2048),
                r(4096, 0, 904, 2048),
                r(0, 2048, 2048, 52),
                r(2048, 2048, 2048, 52),
                r(4096, 2048, 904, 52),
            ]
        );
    }

    /// A dirty rect across tile boundaries updates each touched tile at its own offset.
    #[test]
    fn partial_upload_splits_across_tiles() {
        let grid = Tiles::grid([5000, 2100], 2048);
        assert_eq!(
            Tiles::parts(&grid, r(2000, 2040, 100, 20)),
            [
                (0, [2000, 2040], r(2000, 2040, 48, 8)),
                (1, [0, 2040], r(2048, 2040, 52, 8)),
                (3, [2000, 0], r(2000, 2048, 48, 12)),
                (4, [0, 0], r(2048, 2048, 52, 12)),
            ]
        );
        assert_eq!(
            Tiles::parts(&grid, r(10, 10, 5, 5)),
            [(0, [10, 10], r(10, 10, 5, 5))]
        );
        assert!(Tiles::parts(&grid, r(10, 10, 0, 5)).is_empty());
    }
}
