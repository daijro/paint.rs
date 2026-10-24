//! CSS outer `box-shadow` as Chromium's software raster paints it (`.drop-menu { box-shadow: 2px
//! 2px 5px rgba(0, 0, 0, .55) }`), measured exact on all popup shadow pixels of the menu
//! references:
//! - the mask is Skia's analytic `SkBlurMask::BlurRect` (σ = blur / 2) of the pixel-snapped
//!   border box moved by the offset (`pdn_core::canvas::blur::skia_blur_rect_mask`);
//! - Skia blits it with the legacy `blit_mask_d32_a8_general`: the shadow's premultiplied color
//!   scaled by `m + 1` (`SkAlphaMulQ(c, SkAlpha255To256(m))`), then `d * (256 - sa) >> 8` — a
//!   truncating blend no fixed-function GPU blend state reproduces over an unknown backdrop.
//!
//! So the backdrop is recomputed on the CPU: the shapes already painted in the layer are replayed
//! over the shadow's area (flat rects, and textured quads whose pixels the caller can supply), the
//! blend is applied there and the result is painted as an opaque texture. Pixels under shapes
//! that cannot be replayed (text, other meshes) fall back to GPU blending of the same alpha,
//! which can be 1 off. An owner's image is reused while everything it is made from (geometry,
//! the shapes under it, the versions of the textures they sample) is unchanged.
//! Reference: paintdotjs @ b4e6fc5 (css/menu.css `.drop-menu`)
use std::collections::HashMap;
use std::sync::Arc;

use egui::epaint::{ClippedShape, Mesh, RectShape};
use egui::{Color32, ColorImage, Context, Id, LayerId, Pos2, Rect, Shape, TextureId, Ui};
use pdn_core::canvas::blur::skia_blur_rect_mask;
use pdn_core::pixel::IntRect;

use super::tiles::{self, Tiles};

/// CPU pixels of a texture (straight `ColorImage` as uploaded) by id, for the backdrop replay.
/// The shadow cache assumes a texture's pixels change only through `Tiles::upload` (any other
/// texture it returns keeps its pixels for its id's lifetime); debug builds check every hit.
pub type TexturePixels<'a> = &'a dyn Fn(TextureId) -> Option<Arc<ColorImage>>;

/// A CSS box shadow: `offset` and `blur` in CSS px, `color` unpremultiplied.
#[derive(Clone, Copy, Debug)]
pub struct BoxShadow {
    pub offset: egui::Vec2,
    pub blur: f32,
    pub color: Color32,
}

/// Shadow textures this pass painted, kept so a later shadow can replay them.
#[derive(Clone, Default)]
struct Painted {
    pass: u64,
    textures: Vec<(TextureId, Arc<ColorImage>)>,
}

fn painted_id() -> Id {
    Id::unique("paintrs.kit.shadow.painted")
}

/// Textures that change every frame of a canvas gesture (a pan, a zoom, a drag of moving pixels),
/// set for one pass by [`set_live`].
#[derive(Clone, Default)]
struct Live {
    pass: u64,
    ids: Vec<TextureId>,
}

fn live_id() -> Id {
    Id::unique("paintrs.kit.shadow.live")
}

/// For this pass, shadows and veils treat `ids`' pixels as unknown, so their images (and keys)
/// stay the same while those textures change every frame; a veil draws them back over its box at
/// the backdrop's share with GPU blending instead (within a level or two of the exact blend). The
/// first pass without them replays exactly again. Call before the shadows paint.
pub fn set_live(ctx: &Context, ids: Vec<TextureId>) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(live_id(), Live { pass, ids }));
}

/// This pass's live textures.
fn live(ctx: &Context) -> Vec<TextureId> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<Live>(live_id()))
        .filter(|l| l.pass == pass)
        .map_or_else(Vec::new, |l| l.ids)
}

/// The device-pixel backdrop of `area` (`None` = unknown).
#[derive(Clone)]
struct Backdrop {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    px: Vec<Option<[u8; 3]>>,
    /// Device rect nobody reads (a shadow's box): never replayed, left `None`.
    hole: [i32; 4],
}

impl Backdrop {
    fn bounds(&self) -> [i32; 4] {
        [self.x0, self.y0, self.x0 + self.w, self.y0 + self.h]
    }

    /// `[x0, x1)` on row `y` minus the hole: up to two runs.
    fn runs(&self, y: i32, x0: i32, x1: i32) -> [std::ops::Range<i32>; 2] {
        let [hx0, hy0, hx1, hy1] = self.hole;
        if y < hy0 || y >= hy1 || hx0 >= hx1 {
            return [x0..x1, 0..0];
        }
        [x0..x1.min(hx0), x0.max(hx1)..x1]
    }

    /// Pixels of device rect `[x0, x1) x [y0, y1)` clipped to the area, the hole skipped.
    fn each(&mut self, r: [i32; 4], mut f: impl FnMut(i32, i32, &mut Option<[u8; 3]>)) {
        let [x0, y0, x1, y1] = intersect(r, self.bounds());
        for y in y0..y1 {
            for xs in self.runs(y, x0, x1) {
                for x in xs {
                    let i = ((y - self.y0) * self.w + (x - self.x0)) as usize;
                    f(x, y, &mut self.px[i]);
                }
            }
        }
    }
}

/// egui's premultiplied src-over in an 8-bit blend unit: `s + round(d * (255 - a) / 255)`.
fn gpu_over(src: Color32, dst: [u8; 3]) -> [u8; 3] {
    let a = 255 - src.a() as u32;
    let s = [src.r(), src.g(), src.b()];
    std::array::from_fn(|c| (s[c] as u32 + (dst[c] as u32 * a + 127) / 255).min(255) as u8)
}

fn device_rect(r: Rect, ppp: f32) -> [i32; 4] {
    let s = |v: f32| (v * ppp).round() as i32;
    [s(r.min.x), s(r.min.y), s(r.max.x), s(r.max.y)]
}

fn intersect(a: [i32; 4], b: [i32; 4]) -> [i32; 4] {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

fn replay(b: &mut Backdrop, shape: &Shape, clip: [i32; 4], ppp: f32, pixels: TexturePixels) {
    let unknown = |b: &mut Backdrop, r: Rect| {
        if r.is_positive() {
            let d = [
                (r.min.x * ppp).floor() as i32,
                (r.min.y * ppp).floor() as i32,
                (r.max.x * ppp).ceil() as i32,
                (r.max.y * ppp).ceil() as i32,
            ];
            b.each(intersect(d, clip), |_, _, p| *p = None);
        }
    };
    match shape {
        Shape::Noop => {}
        Shape::Vec(shapes) => {
            for s in shapes {
                replay(b, s, clip, ppp, pixels);
            }
        }
        Shape::Rect(r) if plain_fill(r) => {
            let fill = r.fill;
            if fill.a() == 0 {
                return;
            }
            let r = intersect(device_rect(r.rect, ppp), clip);
            if fill.a() == 255 {
                // `gpu_over` of an opaque source is the source.
                let rgb = Some([fill.r(), fill.g(), fill.b()]);
                b.each(r, |_, _, p| *p = rgb);
                return;
            }
            b.each(r, |_, _, p| *p = p.map(|d| gpu_over(fill, d)));
        }
        Shape::Mesh(mesh) => {
            if !replay_quad(b, mesh, clip, ppp, pixels) {
                unknown(b, mesh.calc_bounds());
            }
        }
        other => unknown(b, other.visual_bounding_rect()),
    }
}

/// A flat, square-cornered, unstroked, untextured rect (`painter.rect_filled`).
fn plain_fill(r: &RectShape) -> bool {
    r.brush.is_none()
        && r.corner_radius == Default::default()
        && r.stroke.is_empty()
        && r.blur_width == 0.0
        && r.angle == 0.0
}

/// How `replay_quad` maps a `Shape::image` quad's device pixels to its texels: NEAREST at the
/// pixel centres. The scene key computes the texels a quad reads with the same map.
struct QuadMap {
    min: Pos2,
    max: Pos2,
    uv0: Pos2,
    uv1: Pos2,
    tint: Color32,
    ppp: f32,
    size: [usize; 2],
}

impl QuadMap {
    /// `mesh` as one axis-aligned quad with one tint (`Mesh::add_rect_with_uv`: left-top,
    /// right-top, left-bottom, right-bottom) over a `size` texture.
    fn new(mesh: &Mesh, ppp: f32, size: [usize; 2]) -> Option<Self> {
        if mesh.vertices.len() != 4 || mesh.indices.len() != 6 || size[0] == 0 || size[1] == 0 {
            return None;
        }
        let v = &mesh.vertices;
        let (min, max) = (v[0].pos, v[3].pos);
        if v[1].pos != Pos2::new(max.x, min.y) || v[2].pos != Pos2::new(min.x, max.y) {
            return None;
        }
        let tint = v[0].color;
        if v.iter().any(|v| v.color != tint) {
            return None;
        }
        Some(QuadMap {
            min,
            max,
            uv0: v[0].uv,
            uv1: v[3].uv,
            tint,
            ppp,
            size,
        })
    }

    /// The quad's device rect.
    fn device(&self) -> [i32; 4] {
        device_rect(Rect::from_min_max(self.min, self.max), self.ppp)
    }

    fn col(&self, x: i32) -> usize {
        let dx = (self.max.x - self.min.x) * self.ppp;
        let fx = (x as f32 + 0.5 - self.min.x * self.ppp) / dx;
        let u = self.uv0.x + (self.uv1.x - self.uv0.x) * fx;
        ((u * self.size[0] as f32).floor() as usize).min(self.size[0] - 1)
    }

    fn row(&self, y: i32) -> usize {
        let dy = (self.max.y - self.min.y) * self.ppp;
        let fy = (y as f32 + 0.5 - self.min.y * self.ppp) / dy;
        let w = self.uv0.y + (self.uv1.y - self.uv0.y) * fy;
        ((w * self.size[1] as f32).floor() as usize).min(self.size[1] - 1)
    }

    /// The texel rect read over device rect `r` (`[x0, y0, x1, y1]`, inside [`Self::device`]):
    /// the maps are monotonic, so the end pixels bound it.
    fn texels(&self, [x0, y0, x1, y1]: [i32; 4]) -> IntRect {
        if x0 >= x1 || y0 >= y1 {
            return IntRect::default();
        }
        let (a, b) = (self.col(x0), self.col(x1 - 1));
        let (c, d) = (self.row(y0), self.row(y1 - 1));
        let (l, t) = (a.min(b), c.min(d));
        IntRect::new(
            l as i32,
            t as i32,
            (a.max(b) - l + 1) as u32,
            (c.max(d) - t + 1) as u32,
        )
    }
}

/// `Shape::image`: one axis-aligned textured quad, sampled NEAREST at pixel centers.
fn replay_quad(
    b: &mut Backdrop,
    mesh: &Mesh,
    clip: [i32; 4],
    ppp: f32,
    pixels: TexturePixels,
) -> bool {
    if mesh.vertices.len() != 4 || mesh.indices.len() != 6 {
        return false;
    }
    let Some(img) = pixels(mesh.texture_id) else {
        return false;
    };
    let Some(map) = QuadMap::new(mesh, ppp, img.size) else {
        return false;
    };
    let tw = img.size[0];
    let tint = map.tint;
    let [x0, y0, x1, y1] = intersect(intersect(map.device(), clip), b.bounds());
    // The texel column of every device column (the same on each row).
    let cols: Vec<usize> = (x0..x1).map(|x| map.col(x)).collect();
    for y in y0..y1 {
        let ty = map.row(y);
        let row = &img.pixels[ty * tw..][..tw];
        let at = (y - b.y0) * b.w - b.x0;
        for xs in b.runs(y, x0, x1) {
            for x in xs {
                let texel = row[cols[(x - x0) as usize]];
                // `mul(v, 255) == v`: an untinted image is its texels.
                let c = if tint == Color32::WHITE {
                    texel
                } else {
                    Color32::from_rgba_premultiplied(
                        mul(texel.r(), tint.r()),
                        mul(texel.g(), tint.g()),
                        mul(texel.b(), tint.b()),
                        mul(texel.a(), tint.a()),
                    )
                };
                let p = &mut b.px[(at + x) as usize];
                *p = match (*p, c.a()) {
                    (_, 255) => Some([c.r(), c.g(), c.b()]),
                    (Some(d), _) => Some(gpu_over(c, d)),
                    (None, _) => None,
                };
            }
        }
    }
    true
}

fn mul(a: u8, b: u8) -> u8 {
    ((a as u32 * b as u32 + 127) / 255) as u8
}

/// The layers painted under and including `me`, in paint order (`GraphicLayers::drain`): the
/// background (the shell), then the areas of a lower order or below `me` in its order (a modal
/// dialog's layer sits over the shell and the dialogs under it).
fn layers_up_to(ctx: &Context, me: LayerId) -> Vec<LayerId> {
    let background = LayerId::background();
    let mut layers = vec![];
    if me != background {
        layers.push(background);
        let areas: Vec<LayerId> = ctx.memory(|m| m.layer_ids().collect());
        let at = areas.iter().position(|&l| l == me);
        for (i, &l) in areas.iter().enumerate() {
            let below = l.order < me.order || (l.order == me.order && at.is_some_and(|a| i < a));
            if l != background && below {
                layers.push(l);
            }
        }
    }
    layers.push(me);
    layers
}

/// What a backdrop replays over `area`: the clipped shapes painted so far in a layer and the layers
/// under it, and the [`tiles::versions`] of the textures their image quads sample. Two equal scenes
/// replay to the same pixels.
#[derive(Clone)]
struct Scene {
    area: [i32; 4],
    ppp: f32,
    shapes: Vec<([i32; 4], Shape)>,
    /// Per image quad: its texture, the texture's version and the texels the quad reads.
    textures: Vec<(TextureId, u64, IntRect)>,
}

impl Scene {
    /// Replays to `old`'s pixels: the same area, scale and shapes, and no texture changed where
    /// these quads read it since `old`'s versions (`tiles::changed_within`), so a brush dab away
    /// from a form keeps its shadow and veil.
    fn same_as(&self, old: &Scene, ctx: &Context) -> bool {
        self.area == old.area
            && self.ppp == old.ppp
            && self.textures.len() == old.textures.len()
            && self.shapes == old.shapes
            && self.textures.iter().zip(&old.textures).all(|(new, old)| {
                let ((id, version, texels), (old_id, old_version, old_texels)) = (*new, *old);
                id == old_id
                    && texels == old_texels
                    && (version == old_version
                        || !tiles::changed_within(ctx, id, old_version, texels))
            })
    }
}

/// A scene and the shadow textures this pass had painted when it was collected.
struct Snapshot {
    scene: Arc<Scene>,
    painted: Vec<(TextureId, Arc<ColorImage>)>,
}

/// The image quads in `shape` (clipped to device rect `clip`, inside the scene's area): each
/// one's texture and the texels `replay_quad` reads there. `size` gives a texture's size.
fn quad_reads(
    shape: &Shape,
    clip: [i32; 4],
    ppp: f32,
    size: &dyn Fn(TextureId) -> Option<[usize; 2]>,
    out: &mut Vec<(TextureId, IntRect)>,
) {
    match shape {
        Shape::Vec(shapes) => {
            for s in shapes {
                quad_reads(s, clip, ppp, size, out);
            }
        }
        Shape::Mesh(mesh) if mesh.vertices.len() == 4 => {
            let id = mesh.texture_id;
            let texels = size(id)
                .and_then(|size| QuadMap::new(mesh, ppp, size))
                .map_or(IntRect::default(), |map| {
                    map.texels(intersect(map.device(), clip))
                });
            out.push((id, texels));
        }
        _ => {}
    }
}

impl Snapshot {
    /// The shapes painted so far in `ui`'s layer and the layers under it over device rect `area`.
    fn collect(ui: &Ui, area: [i32; 4]) -> Self {
        let ctx = ui.ctx();
        let ppp = ctx.pixels_per_point();
        let pass = ctx.cumulative_pass_nr();
        let painted = ctx.data_mut(|d| {
            let p = d.get_temp_mut_or_default::<Painted>(painted_id());
            if p.pass != pass {
                *p = Painted {
                    pass,
                    textures: vec![],
                };
            }
            p.textures.clone()
        });
        let layers = layers_up_to(ctx, ui.layer_id());
        let shapes: Vec<([i32; 4], Shape)> = ctx.graphics(|g| {
            layers
                .iter()
                .filter_map(|&l| g.get(l))
                .flat_map(|list| list.all_entries())
                .filter_map(
                    |ClippedShape {
                         clip_rect, shape, ..
                     }| {
                        let c = intersect(device_rect(*clip_rect, ppp), area);
                        // Only shapes that reach the area: `replay` writes nothing past a
                        // shape's outward-rounded bounds (a toolbar icon under a full-screen
                        // clip stays out of the scene and its key).
                        let b = shape.visual_bounding_rect();
                        let reach = intersect(
                            c,
                            [
                                (b.min.x * ppp).floor() as i32,
                                (b.min.y * ppp).floor() as i32,
                                (b.max.x * ppp).ceil() as i32,
                                (b.max.y * ppp).ceil() as i32,
                            ],
                        );
                        (reach[0] < reach[2] && reach[1] < reach[3]).then(|| (c, shape.clone()))
                    },
                )
                .collect()
        });
        let mut reads = vec![];
        {
            let manager = ctx.tex_manager();
            let manager = manager.read();
            let size = |id| manager.meta(id).map(|m| m.size);
            for (clip, shape) in &shapes {
                quad_reads(shape, *clip, ppp, &size, &mut reads);
            }
        }
        // A live texture's pixels are not replayed (`set_live`): not part of the key.
        let live = live(ctx);
        reads.retain(|(id, _)| !live.contains(id));
        let ids: Vec<TextureId> = reads.iter().map(|r| r.0).collect();
        let textures = (reads.into_iter().zip(tiles::versions(ctx, &ids)))
            .map(|((id, texels), version)| (id, version, texels))
            .collect();
        Snapshot {
            scene: Arc::new(Scene {
                area,
                ppp,
                shapes,
                textures,
            }),
            painted,
        }
    }
}

/// The device pixels of the scene's area as its shapes (and this pass's shadows) leave them;
/// `None` where a shape cannot be replayed, and in `hole`.
fn replay_scene(
    scene: &Scene,
    painted: &[(TextureId, Arc<ColorImage>)],
    hole: [i32; 4],
    pixels: TexturePixels,
) -> Backdrop {
    let [ax, ay, x1, y1] = scene.area;
    let (aw, ah) = ((x1 - ax).max(0), (y1 - ay).max(0));
    let mut back = Backdrop {
        x0: ax,
        y0: ay,
        w: aw,
        h: ah,
        px: vec![None; (aw * ah) as usize],
        hole,
    };
    let start = last_cover(&scene.shapes, scene.area, scene.ppp).unwrap_or(0);
    replay_all(
        &mut back,
        &scene.shapes[start..],
        scene.ppp,
        painted,
        pixels,
    );
    back
}

/// The last of `shapes` that paints all of `area` one opaque colour (a plain fill): replaying
/// sets every pixel there whatever it held, so the shapes before it can't show.
fn last_cover(shapes: &[([i32; 4], Shape)], area: [i32; 4], ppp: f32) -> Option<usize> {
    shapes.iter().rposition(|(clip, shape)| {
        matches!(shape, Shape::Rect(r) if plain_fill(r)
            && r.fill.a() == 255
            && intersect(intersect(device_rect(r.rect, ppp), *clip), area) == area)
    })
}

/// `shapes` replayed over `back` in order, textures from `painted` (this pass's shadows) first.
fn replay_all(
    back: &mut Backdrop,
    shapes: &[([i32; 4], Shape)],
    ppp: f32,
    painted: &[(TextureId, Arc<ColorImage>)],
    pixels: TexturePixels,
) {
    let lookup = |id: TextureId| {
        painted
            .iter()
            .find(|(t, _)| *t == id)
            .map(|(_, img)| img.clone())
            .or_else(|| pixels(id))
    };
    for (clip, shape) in shapes {
        replay(back, shape, *clip, ppp, &lookup);
    }
}

/// `group`'s backdrop given `under`'s: the group's shapes are the underlay's plus what the box
/// painted since (its layer is the last collected), so only those replay over a copy, as replaying
/// is pixel by pixel in shape order. Otherwise the whole group replays.
fn replay_group(
    under: &Backdrop,
    under_scene: &Scene,
    group: &Scene,
    painted: &[(TextureId, Arc<ColorImage>)],
    pixels: TexturePixels,
) -> Backdrop {
    let n = under_scene.shapes.len();
    let continues = group.area == under_scene.area
        && group.ppp == under_scene.ppp
        && group.shapes.get(..n) == Some(&under_scene.shapes[..]);
    // (A box painting its whole area opaque, as a window does, hides the underlay anyway.)
    let Some(added) = group
        .shapes
        .get(n..)
        .filter(|added| continues && last_cover(added, group.area, group.ppp).is_none())
    else {
        return replay_scene(group, painted, [0; 4], pixels);
    };
    let mut back = under.clone();
    replay_all(&mut back, added, group.ppp, painted, pixels);
    back
}

/// A shadow's blur-mask input: σ, the moved box and the screen clip.
type Geometry = (f32, [f32; 4], [i32; 4]);

/// The non-scene inputs of an image.
#[derive(Clone, PartialEq)]
enum Params {
    Shadow {
        geometry: Geometry,
        color: Color32,
        group: u32,
        bx: [i32; 4],
    },
    Veil {
        opaque: [i32; 4],
        a: u32,
    },
}

/// Everything a shadow or veil image is made from.
#[derive(Clone)]
struct Key {
    params: Params,
    scenes: Vec<Arc<Scene>>,
}

impl Key {
    /// `old`'s image is this key's (see [`Scene::same_as`]).
    fn same_as(&self, old: &Key, ctx: &Context) -> bool {
        self.params == old.params
            && self.scenes.len() == old.scenes.len()
            && (self.scenes.iter().zip(&old.scenes)).all(|(new, old)| new.same_as(old, ctx))
    }
}

/// An owner's last image, what it was made from and its textures.
#[derive(Clone)]
struct Entry {
    pass: u64,
    key: Key,
    image: Arc<ColorImage>,
    tiles: Tiles,
    /// A shadow's blur mask: its geometry, area (`[x, y, w, h]`) and coverage.
    mask: Option<(Geometry, [i32; 4], Arc<Vec<u8>>)>,
}

#[derive(Clone, Default)]
struct Cache {
    /// The last pass that used the cache.
    pass: u64,
    entries: HashMap<Id, Entry>,
}

fn cache_id() -> Id {
    Id::unique("paintrs.kit.shadow.cache")
}

/// Takes `owner`'s entry out of the cache. A pass's first take drops the entries the previous
/// using pass did not use, so the cache holds only what is still shown.
fn take(ctx: &Context, owner: Id) -> Option<Entry> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let cache = d.get_temp_mut_or_default::<Cache>(cache_id());
        if cache.pass != pass {
            let last = cache.pass;
            cache.entries.retain(|_, e| e.pass == last);
            cache.pass = pass;
        }
        cache.entries.remove(&owner)
    })
}

/// Paints `owner`'s image for `key` at `at` and records it as painted: last frame's image when
/// the key is unchanged, else `make`'s (uploaded only when its pixels changed).
#[allow(clippy::too_many_arguments)]
fn paint_cached(
    ui: &Ui,
    owner: Id,
    kind: &str,
    key: Key,
    old: Option<Entry>,
    mask: Option<(Geometry, [i32; 4], Arc<Vec<u8>>)>,
    at: Pos2,
    make: impl FnOnce(&[Arc<Scene>]) -> Arc<ColorImage>,
) -> Arc<ColorImage> {
    let ctx = ui.ctx();
    let (image, tiles) = match old {
        Some(e) if key.same_as(&e.key, ctx) => {
            // Debug builds check every hit: a key that misses an input fails here.
            #[cfg(debug_assertions)]
            assert!(
                make(&key.scenes) == e.image,
                "stale {kind} cache for {owner:?}"
            );
            (e.image, e.tiles)
        }
        old => {
            let image = make(&key.scenes);
            // Only the pixels that changed are uploaded (and logged: a veil over a shadow whose
            // ring changed elsewhere keeps its image).
            let changed = old.as_ref().map(|e| changed_rect(&e.image, &image));
            match (old, changed) {
                (Some(e), Some(Some(r))) if r.is_empty() => (e.image, e.tiles),
                (old, changed) => {
                    let mut tiles = old.map(|e| e.tiles).unwrap_or_default();
                    tiles.upload(ctx, &format!("{kind}/{owner:?}"), &image, changed.flatten());
                    (image, tiles)
                }
            }
        }
    };
    let ppp = ctx.pixels_per_point();
    tiles.paint(&ui.painter().with_clip_rect(ctx.content_rect()), at, ppp);
    painted(ctx, &tiles, &image);
    let entry = Entry {
        pass: ctx.cumulative_pass_nr(),
        key,
        image: image.clone(),
        tiles,
        mask,
    };
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(cache_id())
            .entries
            .insert(owner, entry);
    });
    image
}

/// The bounds of the pixels that differ between two images (empty when none do); `None` when
/// their sizes differ.
fn changed_rect(a: &ColorImage, b: &ColorImage) -> Option<IntRect> {
    if a.size != b.size {
        return None;
    }
    let w = a.size[0].max(1);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (y, (ra, rb)) in a.pixels.chunks(w).zip(b.pixels.chunks(w)).enumerate() {
        if ra == rb {
            continue;
        }
        let first = ra.iter().zip(rb).position(|(p, q)| p != q).unwrap_or(0);
        let last = w - ra
            .iter()
            .zip(rb)
            .rev()
            .position(|(p, q)| p != q)
            .unwrap_or(0);
        (x0, x1) = (x0.min(first), x1.max(last));
        (y0, y1) = (y0.min(y), y + 1);
    }
    Some(if x0 == usize::MAX {
        IntRect::default()
    } else {
        IntRect::new(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32)
    })
}

/// What lies under a box before it is painted, for [`paint_veil`].
pub struct Underlay {
    area: Rect,
    snapshot: Snapshot,
}

/// Records the backdrop of `area` (points) before a box with CSS `opacity` paints over it.
pub fn capture_underlay(ui: &Ui, area: Rect) -> Underlay {
    let ppp = ui.ctx().pixels_per_point();
    let area = area.intersect(ui.ctx().content_rect());
    let d = device_rect(area, ppp);
    Underlay {
        area: Rect::from_min_max(
            Pos2::new(d[0] as f32, d[1] as f32) / ppp,
            Pos2::new(d[2] as f32, d[3] as f32) / ppp,
        ),
        snapshot: Snapshot::collect(ui, d),
    }
}

/// CSS `opacity` of the group painted since `under` was captured. Skia composites the group's
/// layer (G, opaque in the box) over the backdrop B with a = round(255·opacity) as
/// `((G·(a + 1)) >> 8) + ((B·(256 − a)) >> 8)` (`SkAlphaMulQ`, then `SkPMSrcOver`; measured exact
/// for every B at 0.75 in software Chromium, and on the colors-compact reference's Colors form at
/// .88: 250 over 47 → 224, 8 → 12, 133 → 121). Where the group's pixels replay, the veil is that
/// opaque result inside the group's opaque box `opaque`; elsewhere (text, the group's shadow) it
/// is B at `1 − opacity` over G, which the GPU blends to within ±1 (⌊B·(255 − a)/255 − ½⌋ in the
/// box centres the error). Paint.NET's own (the DWM's) rounding is unknown: matching the
/// reference compositor is the controller's accepted choice. Pixels whose backdrop could not be
/// replayed keep G.
// calibrate: B-W1 — the DWM's rounding of a 0.75-opaque window.
pub fn paint_veil(
    ui: &Ui,
    owner: Id,
    under: &Underlay,
    opaque: Rect,
    opacity: f32,
    pixels: TexturePixels,
) {
    let a = (opacity * 255.0).round() as u32;
    let alpha = 255 - a;
    let area = under.snapshot.scene.area;
    if alpha == 0 || area[2] <= area[0] || area[3] <= area[1] {
        return;
    }
    let ctx = ui.ctx();
    let [ox0, oy0, ox1, oy1] = device_rect(opaque, ctx.pixels_per_point());
    let live = live(ctx);
    let known = |id| (!live.contains(&id)).then(|| pixels(id)).flatten();
    let pixels: TexturePixels = &known;
    let group = Snapshot::collect(ui, area);
    let owner = owner.with("veil");
    let old = take(ctx, owner);
    let key = Key {
        params: Params::Veil {
            opaque: [ox0, oy0, ox1, oy1],
            a,
        },
        scenes: vec![under.snapshot.scene.clone(), group.scene],
    };
    let make = |scenes: &[Arc<Scene>]| {
        let b = replay_scene(&scenes[0], &under.snapshot.painted, [0; 4], pixels);
        let group = replay_group(&b, &scenes[0], &scenes[1], &group.painted, pixels);
        let w = b.w as usize;
        let mut px = Vec::with_capacity(b.px.len());
        let rows = b.px.chunks_exact(w).zip(group.px.chunks_exact(w));
        for ((backs, gs), y) in rows.zip(b.y0..) {
            let row = (oy0..oy1).contains(&y);
            for ((back, g), x) in backs.iter().zip(gs).zip(b.x0..) {
                let inside = row && (ox0..ox1).contains(&x);
                px.push(match (back, g) {
                    (Some(bk), Some(g)) if inside => {
                        let m = |g: u8, bk: u8| {
                            (((g as u32 * (a + 1)) >> 8) + ((bk as u32 * (256 - a)) >> 8)) as u8
                        };
                        Color32::from_rgb(m(g[0], bk[0]), m(g[1], bk[1]), m(g[2], bk[2]))
                    }
                    (Some(c), _) => {
                        // The GPU rounds G·a/255 + veil: floored by ½ in the box, rounded outside
                        // (under the group's translucent shadow, `B·(1 − opacity·m)`).
                        let m = |v: u8| {
                            let scaled = 2 * v as u32 * alpha;
                            let out = if inside {
                                scaled.saturating_sub(255) / 510
                            } else {
                                (scaled + 255) / 510
                            };
                            out as u8
                        };
                        Color32::from_rgba_premultiplied(m(c[0]), m(c[1]), m(c[2]), alpha as u8)
                    }
                    (None, _) => Color32::TRANSPARENT,
                });
            }
        }
        Arc::new(ColorImage::new([b.w as usize, b.h as usize], px))
    };
    let image = paint_cached(ui, owner, "veil", key, old, None, under.area.min, make);
    if !live.is_empty() {
        let bx = [ox0, oy0, ox1, oy1];
        let id = owner.with("holes");
        let cached = ctx.data(|d| d.get_temp::<Holes>(id));
        let holes = match cached {
            Some((i, b, h)) if Arc::ptr_eq(&i, &image) && b == bx => h,
            _ => {
                let h = Arc::new(holes(&image, [area[0], area[1]], bx));
                ctx.data_mut(|d| d.insert_temp(id, (image, bx, h.clone())));
                h
            }
        };
        paint_live(ui, &under.snapshot.scene, &live, &holes, alpha);
    }
}

/// A veil's image, its opaque box and its `holes` there.
type Holes = (Arc<ColorImage>, [i32; 4], Arc<Vec<[i32; 4]>>);

/// The device rects (`[x0, y0, x1, y1]`) inside `bx` where `image` (its top left at device px
/// `at`) is transparent: where a veil's backdrop could not be replayed.
fn holes(image: &ColorImage, at: [i32; 2], bx: [i32; 4]) -> Vec<[i32; 4]> {
    let [w, h] = image.size.map(|v| v as i32);
    let (x0, x1) = (bx[0].max(at[0]), bx[2].min(at[0] + w));
    let mut out: Vec<[i32; 4]> = vec![];
    // The rects reaching the row above, which a run with the same columns extends.
    let mut open: Vec<usize> = vec![];
    for y in bx[1].max(at[1])..bx[3].min(at[1] + h) {
        let row = &image.pixels[((y - at[1]) * w) as usize..][..w as usize];
        let clear = |x: i32| row[(x - at[0]) as usize].a() == 0;
        let mut next = vec![];
        let mut x = x0;
        while x < x1 {
            if !clear(x) {
                x += 1;
                continue;
            }
            let start = x;
            while x < x1 && clear(x) {
                x += 1;
            }
            let above = open.iter().find(|&&i| out[i][0] == start && out[i][2] == x);
            match above {
                Some(&i) => {
                    out[i][3] = y + 1;
                    next.push(i);
                }
                None => {
                    out.push([start, y, x, y + 1]);
                    next.push(out.len() - 1);
                }
            }
        }
        open = next;
    }
    out
}

/// A live veil's backdrop (`set_live`): the live quads drawn again at `alpha`/255
/// (B·(1 − opacity) over the opaque group) inside the veil's `holes`, where its backdrop could
/// not be replayed; wherever whatever was painted over the canvas replays (another window, its
/// border, an icon), the veil already holds it.
fn paint_live(ui: &Ui, scene: &Scene, live: &[TextureId], holes: &[[i32; 4]], alpha: u32) {
    let ppp = scene.ppp;
    let start = last_cover(&scene.shapes, scene.area, ppp).unwrap_or(0);
    let k = alpha as u8;
    let tint = Color32::from_rgba_premultiplied(k, k, k, k);
    for (clip, shape) in &scene.shapes[start..] {
        let Shape::Mesh(mesh) = shape else { continue };
        if !live.contains(&mesh.texture_id) {
            continue;
        }
        let parts = holes.iter().map(|h| intersect(*clip, *h));
        let mut copy = (**mesh).clone();
        copy.vertices.iter_mut().for_each(|v| v.color = tint);
        let copy = Shape::Mesh(Arc::new(copy));
        for [x0, y0, x1, y1] in parts.filter(|p| p[0] < p[2] && p[1] < p[3]) {
            let r = Rect::from_min_max(
                Pos2::new(x0 as f32, y0 as f32) / ppp,
                Pos2::new(x1 as f32, y1 as f32) / ppp,
            );
            ui.painter().with_clip_rect(r).add(copy.clone());
        }
    }
}

/// Records the textures this pass painted, so a later shadow or veil replays them exactly.
fn painted(ctx: &Context, tiles: &Tiles, image: &Arc<ColorImage>) {
    let textures: Vec<_> = tiles.each_pixels(image).collect();
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Painted>(painted_id())
            .textures
            .extend(textures);
    });
}

/// Paints `shadow` for the border box `rect` (points, already pixel-snapped) into `ui`'s layer,
/// over everything painted there so far. `pixels` supplies the CPU pixels of the textures that
/// may lie under it; `owner` keys the texture.
pub fn paint_box_shadow(ui: &Ui, owner: Id, rect: Rect, shadow: BoxShadow, pixels: TexturePixels) {
    paint_group_box_shadow(ui, owner, rect, shadow, 1.0, pixels);
}

/// `paint_box_shadow` for a box whose group has CSS `opacity` (a translucent window): Skia draws
/// the shadow into the group's layer at its own alpha, then composites the layer at
/// `a = round(255·opacity)` (`SkAlphaMulQ`, `(s·(a + 1)) >> 8`), so the faint tail where the
/// layer holds alpha 1 vanishes.
pub fn paint_group_box_shadow(
    ui: &Ui,
    owner: Id,
    rect: Rect,
    shadow: BoxShadow,
    opacity: f32,
    pixels: TexturePixels,
) {
    let group = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32 + 1;
    let ctx = ui.ctx();
    let live = live(ctx);
    let known = |id| (!live.contains(&id)).then(|| pixels(id)).flatten();
    let pixels: TexturePixels = &known;
    let ppp = ctx.pixels_per_point();
    let screen = device_rect(ctx.content_rect(), ppp);
    let bx = device_rect(rect, ppp);
    let (ox, oy) = (shadow.offset.x * ppp, shadow.offset.y * ppp);
    let moved = [
        bx[0] as f32 + ox,
        bx[1] as f32 + oy,
        bx[2] as f32 + ox,
        bx[3] as f32 + oy,
    ];
    let sigma = shadow.blur / 2.0 * ppp;
    let clip = [
        screen[0],
        screen[1],
        screen[2] - screen[0],
        screen[3] - screen[1],
    ];
    let geometry = (sigma, moved, clip);
    let old = take(ctx, owner);
    let mask = match old.as_ref().and_then(|e| e.mask.clone()) {
        Some(mask) if mask.0 == geometry => mask,
        _ => match skia_blur_rect_mask(sigma, moved, clip) {
            Some((area, mask)) => (geometry, area, Arc::new(mask)),
            None => return,
        },
    };
    let [ax, ay, aw, ah] = mask.1;
    let back = Snapshot::collect(ui, [ax, ay, ax + aw, ay + ah]);
    let key = Key {
        params: Params::Shadow {
            geometry,
            color: shadow.color,
            group,
            bx,
        },
        scenes: vec![back.scene],
    };
    let coverage = mask.2.clone();
    let make = |scenes: &[Arc<Scene>]| {
        // The box's own pixels come out transparent: only the ring around it needs the backdrop.
        let back = replay_scene(&scenes[0], &back.painted, bx, pixels);
        // blit_mask_d32_a8_general with the premultiplied shadow color.
        let color = shadow.color;
        let premul = [color.r(), color.g(), color.b(), color.a()];
        // The scaled source per mask value.
        let source: Vec<[u8; 4]> = (1..=256u32)
            .map(|scale| premul.map(|c| ((((c as u32 * scale) >> 8) * group) >> 8) as u8))
            .collect();
        let mut out = Vec::with_capacity((aw * ah) as usize);
        let rows = coverage
            .chunks_exact(aw as usize)
            .zip(back.px.chunks_exact(aw as usize));
        let pixel = |m: u8, d: &Option<[u8; 3]>| {
            let s = source[m as usize];
            match d {
                Some(d) => {
                    let blend = |c: usize| s[c] + ((d[c] as u32 * (256 - s[3] as u32)) >> 8) as u8;
                    Color32::from_rgb(blend(0), blend(1), blend(2))
                }
                None => Color32::from_rgba_premultiplied(s[0], s[1], s[2], s[3]),
            }
        };
        for ((masks, backs), y) in rows.zip(ay..) {
            // The box's own pixels (the hole) are transparent.
            let hole = if y >= bx[1] && y < bx[3] {
                let at = |x: i32| (x.clamp(ax, ax + aw) - ax) as usize;
                at(bx[0])..at(bx[2]).max(at(bx[0]))
            } else {
                0..0
            };
            let ring = |r: std::ops::Range<usize>| {
                masks[r.clone()]
                    .iter()
                    .zip(&backs[r])
                    .map(|(m, d)| pixel(*m, d))
            };
            out.extend(ring(0..hole.start));
            out.resize(out.len() + hole.len(), Color32::TRANSPARENT);
            out.extend(ring(hole.end..aw as usize));
        }
        Arc::new(ColorImage::new([aw as usize, ah as usize], out))
    };
    let at = Pos2::new(ax as f32 / ppp, ay as f32 / ppp);
    paint_cached(ui, owner, "box-shadow", key, old, Some(mask), at, make);
}

/// Drops the texture of a shadow (or, with `owner.with("veil")`, a veil) no longer shown.
pub fn forget(ctx: &Context, owner: Id) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(cache_id())
            .entries
            .remove(&owner);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M5: a live veil draws the canvas again only where its backdrop is unknown (its image
    /// transparent): rows with the same runs merge, the image's own offset counts, the box clips.
    #[test]
    fn holes_are_the_transparent_runs_in_the_box() {
        let mut img = ColorImage::filled([6, 5], Color32::WHITE);
        for (x, y) in [(1, 1), (2, 1), (1, 2), (2, 2), (4, 2), (5, 4)] {
            img.pixels[y * 6 + x] = Color32::TRANSPARENT;
        }
        let at = [10, 20];
        assert_eq!(
            holes(&img, at, [0, 0, 100, 100]),
            vec![[11, 21, 13, 23], [14, 22, 15, 23], [15, 24, 16, 25]]
        );
        assert_eq!(
            holes(&img, at, [12, 0, 15, 23]),
            vec![[12, 21, 13, 23], [14, 22, 15, 23]]
        );
    }

    /// The upload of a changed shadow or veil covers just the pixels that changed.
    #[test]
    fn changed_rect_bounds_the_differences() {
        let a = ColorImage::filled([6, 4], Color32::BLACK);
        let mut b = a.clone();
        assert_eq!(changed_rect(&a, &b), Some(IntRect::default()));
        b.pixels[6 + 2] = Color32::WHITE;
        b.pixels[2 * 6 + 4] = Color32::WHITE;
        assert_eq!(changed_rect(&a, &b), Some(IntRect::new(2, 1, 3, 2)));
        assert_eq!(
            changed_rect(&a, &ColorImage::filled([4, 6], Color32::BLACK)),
            None
        );
    }

    #[test]
    fn legacy_blit_matches_reference_pixels() {
        // menu-effects-blurs, below the submenu over the white document: mask values 183..1 of
        // the analytic BlurRect give 155, 175, 195, 215, 231, 243, 249, 253, 254.
        let blit = |m: u32, d: u32| {
            let sa = (140 * (m + 1)) >> 8;
            (d * (256 - sa)) >> 8
        };
        let got: Vec<u32> = [183, 147, 109, 73, 43, 22, 10, 3, 1]
            .iter()
            .map(|&m| blit(m, 255))
            .collect();
        assert_eq!(got, [155, 175, 195, 215, 231, 243, 249, 253, 254]);
        // Over the #2F2F2F editor background (menu-file): 28, 32, 35, 39, 42, 44, 45, 46, 46.
        let got: Vec<u32> = [183, 147, 109, 73, 43, 22, 10, 3, 1]
            .iter()
            .map(|&m| blit(m, 47))
            .collect();
        assert_eq!(got, [28, 32, 35, 39, 42, 44, 45, 46, 46]);
    }

    #[test]
    fn replays_flat_rects_and_marks_text_unknown() {
        let mut b = Backdrop {
            x0: 0,
            y0: 0,
            w: 4,
            h: 1,
            px: vec![None; 4],
            hole: [0; 4],
        };
        let none: TexturePixels = &|_| None;
        let clip = [0, 0, 4, 1];
        let rect = |x0: f32, x1: f32, c: Color32| {
            Shape::rect_filled(
                Rect::from_min_max(Pos2::new(x0, 0.0), Pos2::new(x1, 1.0)),
                0.0,
                c,
            )
        };
        replay(
            &mut b,
            &rect(0.0, 4.0, Color32::from_gray(32)),
            clip,
            1.0,
            none,
        );
        replay(
            &mut b,
            &rect(1.0, 3.0, Color32::from_black_alpha(128)),
            clip,
            1.0,
            none,
        );
        assert_eq!(b.px[0], Some([32; 3]));
        assert_eq!(b.px[1], Some([16; 3]));
        replay(
            &mut b,
            &Shape::Vec(vec![rect(3.0, 4.0, Color32::from_gray(9))]),
            clip,
            1.0,
            none,
        );
        assert_eq!(b.px[3], Some([9; 3]));
        let circle = Shape::circle_filled(Pos2::new(0.5, 0.5), 0.4, Color32::RED);
        replay(&mut b, &circle, clip, 1.0, none);
        assert_eq!(b.px[0], None);
    }

    #[test]
    fn replays_image_quads_around_the_hole() {
        let mut b = Backdrop {
            x0: 0,
            y0: 0,
            w: 4,
            h: 4,
            px: vec![None; 16],
            hole: [1, 1, 3, 3],
        };
        let texels = [[200, 0, 0], [0, 200, 0], [0, 0, 200], [9, 9, 9]];
        let img = Arc::new(ColorImage::new(
            [2, 2],
            texels
                .iter()
                .map(|&[r, g, b]| Color32::from_rgb(r, g, b))
                .collect(),
        ));
        let pixels: TexturePixels = &move |_| Some(img.clone());
        let quad = Shape::image(
            TextureId::User(1),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(4.0, 4.0)),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        replay(&mut b, &quad, [0, 0, 4, 4], 1.0, pixels);
        for y in 0..4 {
            for x in 0..4 {
                let hole = (1..3).contains(&x) && (1..3).contains(&y);
                let want = (!hole).then_some(texels[(y / 2) * 2 + x / 2]);
                assert_eq!(b.px[y * 4 + x], want, "({x}, {y})");
            }
        }
    }

    /// A scene keys on the texels `replay_quad` reads (`quad_reads`), exactly: here a scaled quad
    /// with flipped uvs, clipped on every side, at a fractional scale.
    #[test]
    fn quad_reads_are_the_texels_replay_reads() {
        let (tw, th) = (16, 12);
        let img = Arc::new(ColorImage::new(
            [tw, th],
            (0..tw * th)
                .map(|i| Color32::from_rgb((i % tw) as u8, (i / tw) as u8, 0))
                .collect(),
        ));
        let pixels: TexturePixels = &move |_| Some(img.clone());
        let quad = Shape::image(
            TextureId::User(1),
            Rect::from_min_max(Pos2::new(1.3, 2.1), Pos2::new(21.7, 13.9)),
            Rect::from_min_max(Pos2::new(0.9, 0.8), Pos2::new(0.2, 0.1)),
            Color32::WHITE,
        );
        let (ppp, clip) = (1.25, [6, 5, 19, 14]);
        let mut reads = vec![];
        quad_reads(&quad, clip, ppp, &|_| Some([tw, th]), &mut reads);
        let mut b = Backdrop {
            x0: 0,
            y0: 0,
            w: 32,
            h: 24,
            px: vec![None; 32 * 24],
            hole: [0; 4],
        };
        replay(&mut b, &quad, clip, ppp, pixels);
        let read: Vec<[i32; 2]> = (b.px.iter().flatten())
            .map(|p| [p[0] as i32, p[1] as i32])
            .collect();
        let (x0, x1) = (
            read.iter().map(|p| p[0]).min(),
            read.iter().map(|p| p[0]).max(),
        );
        let (y0, y1) = (
            read.iter().map(|p| p[1]).min(),
            read.iter().map(|p| p[1]).max(),
        );
        let (x0, x1, y0, y1) = (x0.unwrap(), x1.unwrap(), y0.unwrap(), y1.unwrap());
        let want = IntRect::new(x0, y0, (x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32);
        assert_eq!(reads, [(TextureId::User(1), want)]);
    }

    /// A group with fewer shapes than its underlay (egui only appends today) replays alone.
    #[test]
    fn a_group_shorter_than_its_underlay_replays_alone() {
        let fill = |g| {
            let r = Rect::from_min_max(Pos2::ZERO, Pos2::new(2.0, 1.0));
            (
                [0, 0, 2, 1],
                Shape::rect_filled(r, 0.0, Color32::from_gray(g)),
            )
        };
        let scene = |shapes| Scene {
            area: [0, 0, 2, 1],
            ppp: 1.0,
            shapes,
            textures: vec![],
        };
        let (under, group) = (scene(vec![fill(1), fill(2)]), scene(vec![fill(3)]));
        let none: TexturePixels = &|_| None;
        let back = replay_scene(&under, &[], [0; 4], none);
        let px = replay_group(&back, &under, &group, &[], none).px;
        assert_eq!(px, [Some([3; 3]); 2]);
    }
}
