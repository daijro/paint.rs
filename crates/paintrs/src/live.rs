//! Live editing in a Playground session (web only; github.com/daijro/openplayground), after PhotoCraft's (MIT).
//! Playground's `shell/live.js` drives it through [`web::attach`], which offers it only inside Playground; nothing here
//! runs until the shell connects. The shared document is the set of images open in the session (the tabs), as a
//! checkpoint ([`shared`]): each image's size, resolution and layers as canonical JSON without personal views (the
//! selection, the active layer, the view), each layer followed by its pixels in 64×64 tiles, so a brush stroke changes
//! only the bytes of the tiles it touched. An image opened here while connected joins the set; one someone else opened
//! arrives as a tab in the background. A remote change merges into the open images at the end of a quiet frame
//! ([`step`]), keeping this person's views, and is no step of the native history. While connected, Undo and Redo of
//! those images go to the session ([`undo`]), which takes back only this person's edits. Each person's image, pointer,
//! active layer, selection bounds and the gesture under way go to the others ([`presence`]), who see them on the
//! canvas, as dots on its tab and on the Layers row of their layer ([`peers`]).
//!
//! What another person sends is checked before anything is decoded or allocated, and bounded: at most [`MAX_IMAGES`]
//! images of [`MAX_LAYERS`] layers and [`MAX_PIXELS`] pixel bytes in all. Reading it never panics, whatever it holds.
//! Taking a set in reserves its memory fallibly first, so one that doesn't fit is an error that changes nothing. This
//! person's images pass the same checks to join the set ([`check_image`]): one that doesn't stays open here alone,
//! never proposed, replaced or closed by the session, and says why on its canvas ([`Why`]). Nothing the session holds
//! is replaced without a checkpoint of it, so this person's unsent work always merges in or goes to the shell as a
//! draft.
//!
//! Closing the tab of an image this person opened, which no one else changed, takes it out of the set; any other
//! image closed here stays in the set, hidden here (nothing reopens it). Whether anyone else changed it is a guess: the
//! shell doesn't say whose a change is, so this person's own session Undo or Redo of it, or a draft of theirs opened
//! again, counts as someone else's. Undo and Redo of a shared image go to the session, whose Undo is the set's: it may
//! take back an edit in another tab, which then shows. One that takes an image this person opened out of the set leaves
//! it open here alone, with what they did in it. While the shell's session isn't live (catching up, paused,
//! read-only), they go to it and do nothing, as the shell's contract has them.
//!
//! Checkpoint layout (integers little-endian):
//! - `paintrs-live 1\n`;
//! - `{"documents": [...]}`, keys sorted, the images in increasing `id`, each `{"id", "name", "width", "height",
//!   "resolution", "layers"}`, its layers bottom first, each `{"id", "name", "visible", "background", "opacity",
//!   "blend", "text", "tiles": []}`; cut after each layer's `"tiles":[]` ([`pieces`]) into u32 n, n bytes pieces,
//!   each followed by that layer's tiles: u32 count, then per tile in rows from the top left u32 n, n bytes: `0 r g b
//!   a` for a tile of one colour, else the tile as a PNG (RGBA, 8 bits); the last piece by u32::MAX instead.

// The shared template (github.com/daijro/openplayground, patches/live/playground_live.rs).
#[path = "live/playground_live.rs"]
#[allow(dead_code)]
mod template;

#[cfg(any(target_arch = "wasm32", test))]
mod peers;
#[cfg(test)]
mod tests;

#[cfg(any(target_arch = "wasm32", test))]
pub use peers::draw;

use pdn_core::app::AppWorkspace;
use pdn_core::document::Document;
use pdn_core::document::layer::{BlendMode, Layer, LayerProperties};
use pdn_core::document::surface::Surface;
use pdn_core::document::text_layer::TextSpec;
use pdn_core::document::workspace::DocumentWorkspace;
use pdn_core::history::HistoryMemento;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

/// The shell's document limit (`shell/document-delta.js`).
pub const MAX_BYTES: usize = 128 << 20;
const MAGIC: &[u8] = b"paintrs-live 1\n";
const TILE: u32 = 64;
/// The largest image side taken in: the largest this build makes.
const MAX_SIDE: u32 = pdn_core::io::ops::MAX_SIDE;
/// The most tiles a checkpoint holds, counted before they are read: a tile is at least one pixel, an edge tile at least
/// a 64th of its layer's width or height, so [`MAX_PIXELS`] of layers come to about 2 million.
const MAX_TILES: usize = 1 << 22;
/// The most images in a set and layers in an image: every peer installs them all, a tab and a Layers row each.
const MAX_IMAGES: usize = 64;
const MAX_LAYERS: usize = 1000;
/// The most pixel bytes of a whole set (every layer of every image, 4 bytes a pixel), as shared and as taken in. One
/// limit for both, so a set people built up an image at a time stays one a late joiner can take in at once; and with
/// the workspaces' own surfaces it fits a 32-bit (wasm) address space.
const MAX_PIXELS: u64 = 512 << 20;
/// Ids stay exact in JavaScript, and far from the end of the layer id counter.
const MAX_ID: u64 = 1 << 52;
/// The longest image or layer name, and the longest text of a text layer and its font family name.
const MAX_NAME: usize = 255;
const MAX_TEXT: usize = 1 << 16;
/// Resolutions and text DPIs taken in, in pixels per inch.
const RESOLUTIONS: std::ops::RangeInclusive<f64> = 0.01..=100_000.0;
/// How soon after this person's Undo or Redo goes to the session the change it brings is taken in, at most: then that
/// change shows its image.
const FOCUS_MS: f64 = 2000.0;
/// The history entry an image starts from after a change made elsewhere.
const HISTORY: (&str, &str) = (
    "liveSession.historyMementoName",
    "assets/icons/menu_file_open_icon.png",
);

fn invalid(e: impl std::fmt::Display) -> String {
    format!("Invalid shared image: {e}")
}

fn too_large(bytes: u64, limit: u64) -> String {
    format!(
        "The images shared live are {} MiB, over the {} MiB limit. Close some, or make them smaller (fewer layers or a smaller canvas), to edit live.",
        bytes >> 20,
        limit >> 20
    )
}

/// A name as the session shares it: no control or bidirectional-override characters (they would garble a tab title or
/// a Layers row), and for an image no path separators either (it is a file name), at most [`MAX_NAME`] characters.
fn clean_name(name: &str, file: bool) -> String {
    let hidden = |c: char| matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    let name: String = name
        .chars()
        .filter(|c| !hidden(*c))
        .map(|c| {
            if c.is_control() || (file && matches!(c, '/' | '\\')) {
                '_'
            } else {
                c
            }
        })
        .take(MAX_NAME)
        .collect();
    name
}

// ---------------------------------------------------------------- tiles

/// A layer's tiles, encoded.
type Tiles = Rc<[Rc<[u8]>]>;

/// The rectangle of tile `i` in a w×h image: x, y, width, height.
fn tile_rect(w: u32, h: u32, i: usize) -> (u32, u32, u32, u32) {
    let across = w.div_ceil(TILE);
    let (x, y) = (i as u32 % across * TILE, i as u32 / across * TILE);
    (x, y, TILE.min(w - x), TILE.min(h - y))
}

fn tile_count(w: u32, h: u32) -> usize {
    (w.div_ceil(TILE) * h.div_ceil(TILE)) as usize
}

/// The pixels of `rect` in an image `w` wide, row after row.
fn cut(raw: &[u8], w: u32, (x, y, tw, th): (u32, u32, u32, u32)) -> Vec<u8> {
    let (w, x, tw) = (w as usize, x as usize, tw as usize);
    (y as usize..(y + th) as usize)
        .flat_map(|row| &raw[(row * w + x) * 4..(row * w + x + tw) * 4])
        .copied()
        .collect()
}

/// `px` (a tile's pixels) into `rect` of an image `w` wide.
fn paste(raw: &mut [u8], w: u32, (x, y, tw, th): (u32, u32, u32, u32), px: &[u8]) {
    let (w, tw) = (w as usize, tw as usize);
    for row in 0..th as usize {
        let at = ((y as usize + row) * w + x as usize) * 4;
        raw[at..at + tw * 4].copy_from_slice(&px[row * tw * 4..(row + 1) * tw * 4]);
    }
}

/// One tile's bytes (see the module's layout).
fn encode(px: &[u8], tw: u32, th: u32) -> Rc<[u8]> {
    use image::ImageEncoder as _;
    if px.as_chunks::<4>().0.iter().all(|p| p[..] == px[..4]) {
        return [&[0], &px[..4]].concat().into();
    }
    let mut png = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        &mut png,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Adaptive,
    );
    encoder
        .write_image(px, tw, th, image::ExtendedColorType::Rgba8)
        .expect("encoding a PNG in memory");
    png.into()
}

fn decode(bytes: &[u8], tw: u32, th: u32) -> Result<Vec<u8>, String> {
    if let [0, rgba @ ..] = bytes {
        return if rgba.len() == 4 {
            Ok(rgba.repeat((tw * th) as usize))
        } else {
            Err(invalid("tile"))
        };
    }
    // The size in its header first: a PNG from someone else could claim any size.
    let size = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|n| u32::from_be_bytes(n.try_into().unwrap_or_default()))
    };
    if (size(16), size(20)) != (Some(tw), Some(th)) {
        return Err(invalid("tile size"));
    }
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(invalid)?
        .into_rgba8();
    if img.dimensions() != (tw, th) {
        return Err(invalid("tile size"));
    }
    Ok(img.into_raw())
}

/// The three-way merge of one tile's pixels, both sides having changed it: each pixel takes the side that changed it;
/// a pixel both changed takes `current`'s, which goes on after `edited`, so both strokes stay and the later is on top.
fn merge_pixels(r: (u32, u32, u32, u32), b: &[u8], c: &[u8], e: &[u8]) -> Result<Rc<[u8]>, String> {
    let (pb, pc, pe) = (
        decode(b, r.2, r.3)?,
        decode(c, r.2, r.3)?,
        decode(e, r.2, r.3)?,
    );
    let px: Vec<u8> = (pb.as_chunks::<4>().0.iter())
        .zip(pc.as_chunks::<4>().0)
        .zip(pe.as_chunks::<4>().0)
        .flat_map(|((b, c), e)| if c != b { c } else { e })
        .copied()
        .collect();
    Ok(encode(&px, r.2, r.3))
}

// ---------------------------------------------------------------- the checkpoint

/// A checkpoint taken apart: the set of images (each layer's `tiles` empty) and every layer's tiles by (image id,
/// layer id).
#[derive(Clone, Default)]
struct Parts {
    doc: Value,
    tiles: HashMap<(u64, u64), Tiles>,
}

/// The images of a set, by id.
fn documents(set: &Value) -> impl Iterator<Item = (u64, &Value)> {
    set["documents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| Some((d["id"].as_u64()?, d)))
}

/// An image's layers, by id.
fn layers(image: &Value) -> impl Iterator<Item = (u64, &Value)> {
    image["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| Some((l["id"].as_u64()?, l)))
}

/// The layers of a set in checkpoint order, as (image id, layer id).
fn order(set: &Value) -> Vec<(u64, u64)> {
    documents(set)
        .flat_map(|(uid, image)| layers(image).map(move |(lid, _)| (uid, lid)))
        .collect()
}

/// `template::canonical(header)` cut after each `"tiles"` value, in the order [`order`] meets the layers.
fn pieces(header: &Value) -> Vec<Vec<u8>> {
    fn last(out: &mut [Vec<u8>]) -> &mut Vec<u8> {
        out.last_mut().expect("a piece")
    }
    fn walk(v: &Value, out: &mut Vec<Vec<u8>>) {
        match v {
            Value::Object(m) => {
                last(out).push(b'{');
                for (i, (key, v)) in m
                    .iter()
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .into_iter()
                    .enumerate()
                {
                    let piece = last(out);
                    if i > 0 {
                        piece.push(b',');
                    }
                    piece.extend(template::canonical(&Value::String(key.clone())));
                    piece.push(b':');
                    walk(v, out);
                    if key == "tiles" && v.is_array() {
                        out.push(Vec::new());
                    }
                }
                last(out).push(b'}');
            }
            Value::Array(items) => {
                last(out).push(b'[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        last(out).push(b',');
                    }
                    walk(v, out);
                }
                last(out).push(b']');
            }
            scalar => last(out).extend(template::canonical(scalar)),
        }
    }
    let mut out = vec![Vec::new()];
    walk(header, &mut out);
    out
}

fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend((bytes.len() as u32).to_le_bytes());
    out.extend(bytes);
}

/// The canonical bytes of `parts` (see the module's layout).
fn write(parts: &Parts) -> Result<Vec<u8>, String> {
    let json = pieces(&parts.doc);
    let layers = order(&parts.doc);
    if json.len() != layers.len() + 1 {
        return Err(invalid("layers"));
    }
    let mut out = MAGIC.to_vec();
    for (piece, key) in json.iter().zip(&layers) {
        put(&mut out, piece);
        let tiles = parts
            .tiles
            .get(key)
            .ok_or_else(|| invalid("tiles missing"))?;
        out.extend((tiles.len() as u32).to_le_bytes());
        for tile in tiles.iter() {
            put(&mut out, tile);
        }
        if out.len() > MAX_BYTES {
            return Err(too_large(out.len() as u64, MAX_BYTES as u64));
        }
    }
    put(&mut out, json.last().map_or(&[], Vec::as_slice));
    out.extend(u32::MAX.to_le_bytes());
    Ok(out)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| invalid("truncated"))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().map_err(invalid)?,
        ))
    }
}

/// An image's size, checked.
fn size(image: &Value) -> Result<(u32, u32), String> {
    let side = |k: &str| {
        image[k]
            .as_u64()
            .filter(|n| (1..=u64::from(MAX_SIDE)).contains(n))
            .map(|n| n as u32)
    };
    side("width")
        .zip(side("height"))
        .ok_or_else(|| invalid("size"))
}

/// A layer's properties and text, checked.
fn layer_of(l: &Value) -> Result<(LayerProperties, Option<TextSpec>), String> {
    let blend = l["blend"].as_str().ok_or_else(|| invalid("blend mode"))?;
    let blend_mode = BlendMode::from_value(blend);
    if blend_mode.value() != blend {
        return Err(invalid(format!("blend mode `{blend}`")));
    }
    let properties = LayerProperties {
        name: l["name"]
            .as_str()
            .filter(|n| clean_name(n, false) == *n)
            .ok_or_else(|| invalid("layer name"))?
            .to_owned(),
        visible: l["visible"].as_bool().ok_or_else(|| invalid("visible"))?,
        is_background: l["background"]
            .as_bool()
            .ok_or_else(|| invalid("background"))?,
        opacity: l["opacity"]
            .as_u64()
            .and_then(|o| u8::try_from(o).ok())
            .ok_or_else(|| invalid("opacity"))?,
        blend_mode,
    };
    let text: Option<TextSpec> = match &l["text"] {
        Value::Null => None,
        spec => Some(serde_json::from_value(spec.clone()).map_err(invalid)?),
    };
    // A text layer re-renders when someone edits it: only what the Text tool itself would make, written as this build
    // writes it (missing or unknown fields would change when anyone's next edit writes the image out again).
    if let Some(t) = &text {
        if serde_json::to_value(t).ok().as_ref() != Some(&l["text"]) {
            return Err(invalid("text layer"));
        }
        let (s, (x, y)) = (&t.settings, t.origin);
        if t.text.chars().count() > MAX_TEXT
            || s.family.chars().count() > MAX_NAME
            || !(1.0..=2000.0).contains(&s.size)
            || !RESOLUTIONS.contains(&t.dpi)
            || !(x.abs() <= 1e7 && y.abs() <= 1e7)
        {
            return Err(invalid("text layer"));
        }
    }
    Ok((properties, text))
}

/// Whether `v` is an object with exactly the keys `keys`.
fn shaped(v: &Value, keys: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|m| m.len() == keys.len() && keys.iter().all(|k| m.contains_key(*k)))
}

/// Whether `set` is a set of images every peer can install, in the shape this build writes: ids in order, sizes in
/// range, exactly the known fields, layers that read, each with the tiles its image's size has.
fn check(parts: &Parts) -> Result<(), String> {
    let doc = &parts.doc;
    // Another person's data: read with `get` and `Value`'s own indexing (null when missing), never a `Map` index, which
    // panics on a missing key.
    let ids: Option<Vec<u64>> = doc
        .as_object()
        .filter(|m| m.len() == 1)
        .and_then(|m| m.get("documents")?.as_array())
        .and_then(|d| d.iter().map(|d| d["id"].as_u64()).collect());
    if !ids.is_some_and(|ids| {
        ids.len() <= MAX_IMAGES
            && ids.windows(2).all(|w| w[0] < w[1])
            && ids.last().is_none_or(|id| *id < MAX_ID)
    }) {
        return Err(invalid("not a set of at most 64 images in id order"));
    }
    // In u64: a 32-bit `usize` (wasm) can't hold every size a checkpoint may claim.
    let mut pixels = 0u64;
    for (uid, image) in documents(doc) {
        pixels += check_image(image, |lid| parts.tiles.get(&(uid, lid)).map(|t| t.len()))?;
    }
    if pixels > MAX_PIXELS {
        Err(too_large(pixels, MAX_PIXELS))
    } else {
        Ok(())
    }
}

/// One image of a set, checked as [`check`] checks it (`tiles`: how many tiles a layer of it has, by id): its pixel
/// bytes. This person's images go through it too before they are shared ([`entry`]), so the set never holds one that
/// the others would refuse.
fn check_image(image: &Value, tiles: impl Fn(u64) -> Option<usize>) -> Result<u64, String> {
    let (w, h) = size(image)?;
    if !shaped(
        image,
        &["height", "id", "layers", "name", "resolution", "width"],
    ) {
        return Err(invalid("image fields"));
    }
    if !image["resolution"]
        .as_f64()
        .is_some_and(|r| RESOLUTIONS.contains(&r))
        || !(image["name"].is_null()
            || image["name"]
                .as_str()
                .is_some_and(|n| clean_name(n, true) == n))
    {
        return Err(invalid("resolution or name"));
    }
    let all = image["layers"]
        .as_array()
        .ok_or_else(|| invalid("layers"))?;
    let mut seen = HashSet::new();
    if all.is_empty()
        || all.len() > MAX_LAYERS
        || layers(image).count() != all.len()
        || !layers(image).all(|(lid, _)| lid < MAX_ID && seen.insert(lid))
    {
        return Err(invalid("layer ids"));
    }
    let fields = [
        "background",
        "blend",
        "id",
        "name",
        "opacity",
        "text",
        "tiles",
        "visible",
    ];
    for (lid, l) in layers(image) {
        if !shaped(l, &fields) || l["tiles"] != json!([]) {
            return Err(invalid("layer fields"));
        }
        layer_of(l)?;
        if tiles(lid) != Some(tile_count(w, h)) {
            return Err(invalid("tiles"));
        }
    }
    Ok(pixels(w, h, all.len()))
}

/// A canonical checkpoint taken apart; anything else is refused. Tiles are checked as they decode.
fn read(bytes: &[u8]) -> Result<Parts, String> {
    if bytes.len() > MAX_BYTES {
        return Err(too_large(bytes.len() as u64, MAX_BYTES as u64));
    }
    let mut r = Reader { bytes, at: 0 };
    if r.take(MAGIC.len())? != MAGIC {
        return Err(invalid("not a paint.rs live document"));
    }
    let (mut json, mut sections, mut total) = (Vec::new(), Vec::new(), 0usize);
    loop {
        let n = r.u32()? as usize;
        json.push(r.take(n)?);
        let count = r.u32()?;
        if count == u32::MAX {
            break;
        }
        total = total.saturating_add(count as usize);
        if total > MAX_TILES {
            return Err(invalid("too many tiles"));
        }
        let tiles: Result<Vec<Rc<[u8]>>, String> = (0..count)
            .map(|_| {
                let n = r.u32()? as usize;
                Ok(Rc::from(r.take(n)?))
            })
            .collect();
        sections.push(Tiles::from(tiles?));
    }
    if r.at != bytes.len() {
        return Err(invalid("trailing bytes"));
    }
    let doc = template::parse(&json.concat())?;
    if pieces(&doc)
        .iter()
        .map(Vec::as_slice)
        .ne(json.iter().copied())
    {
        return Err(invalid("noncanonical document"));
    }
    let keys = order(&doc);
    if keys.len() != sections.len() {
        return Err(invalid("tiles missing"));
    }
    let parts = Parts {
        tiles: keys.into_iter().zip(sections).collect(),
        doc,
    };
    check(&parts)?;
    Ok(parts)
}

/// The three-way merge of checkpoints: the set merges per image (by id: two opened at once are two), each image per
/// field and per layer (by id), the pixels per tile, and a tile both sides changed pixel by pixel ([`merge_pixels`]).
/// A text layer's tiles are its rendered text: both sides changing them is a conflict. A layer one side deleted and
/// the other painted is a conflict, and so is a new canvas size against any other change of that image's pixels.
/// `current` is the side not yet accepted, `edited` the accepted change.
fn merge(base: &[u8], current: &[u8], edited: &[u8]) -> Result<(Vec<u8>, Parts), String> {
    let (b, c, e) = (read(base)?, read(current)?, read(edited)?);
    let mut doc = template::merge(&b.doc, &c.doc, &e.doc, &[])?;
    if let Some(docs) = doc["documents"].as_array_mut() {
        docs.sort_by_key(|d| d["id"].as_u64());
    }
    let sizes = |p: &Parts| -> HashMap<u64, (u32, u32)> {
        documents(&p.doc)
            .filter_map(|(uid, d)| Some((uid, size(d).ok()?)))
            .collect()
    };
    let (bs, cs, es) = (sizes(&b), sizes(&c), sizes(&e));
    let mut tiles = HashMap::new();
    for (uid, image) in documents(&doc) {
        let wh = size(image)?;
        for (lid, layer) in layers(image) {
            let key = (uid, lid);
            let side = |p: &Parts, s: &HashMap<u64, (u32, u32)>| {
                Some((p.tiles.get(&key)?.clone(), *s.get(&uid)?))
            };
            let (tb, tc, te) = (side(&b, &bs), side(&c, &cs), side(&e, &es));
            let same = |x: &Option<(Tiles, (u32, u32))>, y: &Option<(Tiles, (u32, u32))>| {
                x.as_ref().map(|(t, s)| (&**t, s)) == y.as_ref().map(|(t, s)| (&**t, s))
            };
            let merged = if same(&te, &tb) || same(&tc, &te) {
                tc.or(te)
            } else if same(&tc, &tb) {
                te
            } else {
                match (tb, tc, te) {
                    (Some(_), Some(_), Some(_)) if !layer["text"].is_null() => {
                        return Err(format!("Conflict at /documents[{uid}]/layers[{lid}]/text"));
                    }
                    (Some((tb, sb)), Some((tc, sc)), Some((te, se)))
                        if sb == sc && sc == se && tb.len() == tc.len() && tc.len() == te.len() =>
                    {
                        let out: Result<Vec<_>, String> = (tb.iter().zip(tc.iter()).zip(te.iter()))
                            .enumerate()
                            .map(|(i, ((b, c), e))| match (b, c, e) {
                                (b, c, e) if e == b || c == e => Ok(c.clone()),
                                (b, c, e) if c == b => Ok(e.clone()),
                                (b, c, e) => merge_pixels(tile_rect(sb.0, sb.1, i), b, c, e),
                            })
                            .collect();
                        Some((Tiles::from(out?), sb))
                    }
                    _ => {
                        return Err(format!(
                            "Conflict at /documents[{uid}]: the canvas size changed"
                        ));
                    }
                }
            };
            let (t, s) = merged.ok_or_else(|| invalid("tiles missing"))?;
            if s != wh {
                return Err(format!(
                    "Conflict at /documents[{uid}]: the canvas size changed"
                ));
            }
            tiles.insert(key, t);
        }
    }
    // A layer deleted on one side and painted on the other.
    for (key, t) in &b.tiles {
        if !tiles.contains_key(key)
            && [&c, &e]
                .iter()
                .any(|p| p.tiles.get(key).is_some_and(|x| **x != **t))
        {
            return Err(format!(
                "Conflict at /documents[{}]/layers[{}]",
                key.0, key.1
            ));
        }
    }
    let parts = Parts { doc, tiles };
    check(&parts)?;
    // Every tile the base did not have decodes: a merged set is recorded for everyone, and one that fails to install
    // would fail on every peer for good.
    for (uid, image) in documents(&parts.doc) {
        let (w, h) = size(image)?;
        for (lid, _) in layers(image) {
            let had = b.tiles.get(&(uid, lid));
            let tiles = parts
                .tiles
                .get(&(uid, lid))
                .ok_or_else(|| invalid("tiles missing"))?;
            for (i, t) in tiles.iter().enumerate() {
                if had.is_none_or(|had| had.get(i) != Some(t)) {
                    let r = tile_rect(w, h, i);
                    decode(t, r.2, r.3)?;
                }
            }
        }
    }
    let bytes = write(&parts)?;
    Ok((bytes, parts))
}

// ---------------------------------------------------------------- the images, kept encoded

/// A layer's tiles as last encoded or installed: the surface revision they show, and per tile a hash of its pixels (to
/// find the tiles that changed without keeping a copy of the layer) and its bytes.
struct Cached {
    revision: u64,
    size: (u32, u32),
    hashes: Vec<u64>,
    tiles: Tiles,
}

/// A tile's pixels, hashed.
fn hash(px: &[u8]) -> u64 {
    use std::hash::Hasher as _;
    let mut h = std::hash::DefaultHasher::new();
    h.write(px);
    h.finish()
}

/// What an image's part of the set was made from: when it is unchanged, nothing is encoded again.
#[derive(Clone, PartialEq)]
struct Sig {
    name: Option<String>,
    size: (u32, u32),
    resolution: u64,
    layers: Vec<(u64, LayerProperties, Option<TextSpec>, u64)>,
}

/// Whether `ws` is still what `seen` was made from, compared in place: nothing is cloned on a frame where nothing
/// changed.
fn unchanged(seen: Option<&Sig>, ws: &DocumentWorkspace) -> bool {
    let Some(s) = seen else { return false };
    let d = &ws.document;
    s.name == ws.file_name
        && s.size == (d.width, d.height)
        && s.resolution == d.resolution.to_bits()
        && s.layers.len() == d.layers.size()
        && (s.layers.iter().zip(d.layers.iter())).all(|(a, l)| {
            a.0 == l.id && a.3 == l.surface.revision() && a.1 == l.properties && a.2 == l.text
        })
}

fn sig(ws: &DocumentWorkspace) -> Sig {
    let d = &ws.document;
    Sig {
        name: ws.file_name.clone(),
        size: (d.width, d.height),
        resolution: d.resolution.to_bits(),
        layers: d
            .layers
            .iter()
            .map(|l| {
                (
                    l.id,
                    l.properties.clone(),
                    l.text.clone(),
                    l.surface.revision(),
                )
            })
            .collect(),
    }
}

/// An image's part of the set: its JSON, its layers' tiles, its pixel bytes (every layer's) and its bytes in a
/// checkpoint (its tiles, its JSON and their lengths).
struct Entry {
    doc: Value,
    tiles: HashMap<u64, Tiles>,
    pixels: u64,
    bytes: usize,
}

/// Why an image open here isn't shared.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Why {
    /// It has over [`MAX_LAYERS`] layers.
    Layers,
    /// It is over [`MAX_PIXELS`] on its own.
    Size,
    /// Its tiles come to over [`MAX_BYTES`] on its own.
    Bytes,
    /// Its size, resolution or a text layer is outside what the others take in ([`check_image`]).
    Format,
    /// It would take the set past a limit; the set counts the images closed here too.
    Full,
    /// It was shared, and a change here took it past a limit: the set keeps its version from before.
    Left,
    /// It was shared, and this person's Undo took it out of the set (it was what they added last): it stays open here
    /// with what they did in it.
    Removed,
}

impl Why {
    /// What the canvas says of it (`peers`).
    fn text(self) -> String {
        pdn_core::i18n::t(match self {
            Why::Layers => "liveSession.notShared.layers",
            Why::Size | Why::Bytes => "liveSession.notShared.size",
            Why::Format => "liveSession.notShared.format",
            Why::Full => "liveSession.notShared.full",
            Why::Left => "liveSession.notShared.left",
            Why::Removed => "liveSession.notShared.removed",
        })
    }
}

/// The tiles of `layer` (of image `uid`), encoding only those whose pixels changed since `cache` last saw it.
fn tiles_of(cache: &mut HashMap<(u64, u64), Cached>, uid: u64, layer: &Layer) -> Tiles {
    let s = &layer.surface;
    let (w, h, raw) = (s.width(), s.height(), s.data());
    let known = cache.get(&(uid, layer.id)).filter(|c| c.size == (w, h));
    if let Some(c) = known.filter(|c| c.revision == s.revision()) {
        return c.tiles.clone();
    }
    let mut hashes = Vec::with_capacity(tile_count(w, h));
    let tiles: Vec<Rc<[u8]>> = (0..tile_count(w, h))
        .map(|i| {
            let r = tile_rect(w, h, i);
            let px = cut(raw, w, r);
            let x = hash(&px);
            hashes.push(x);
            match known.and_then(|c| Some((c.hashes.get(i)?, c.tiles.get(i)?))) {
                Some((h, t)) if *h == x => t.clone(),
                _ => encode(&px, r.2, r.3),
            }
        })
        .collect();
    let tiles = Tiles::from(tiles);
    let cached = Cached {
        revision: s.revision(),
        size: (w, h),
        hashes,
        tiles: tiles.clone(),
    };
    cache.insert((uid, layer.id), cached);
    tiles
}

/// The pixel bytes of `layers` layers of w×h.
fn pixels(w: u32, h: u32, layers: usize) -> u64 {
    u64::from(w) * u64::from(h) * 4 * layers as u64
}

/// An image's bytes in a checkpoint: its JSON, each layer's piece and tile count lengths, and its tiles with theirs.
fn bytes_of<'a>(doc: &Value, tiles: impl Iterator<Item = &'a Tiles>) -> usize {
    let layers = doc["layers"].as_array().map_or(0, Vec::len);
    let tiles: usize = tiles
        .map(|t| t.iter().map(|t| t.len() + 4).sum::<usize>())
        .sum();
    template::canonical(doc).len() + 8 * layers + 8 + tiles
}

/// The shared part of `ws` as the set's image `uid`. It must be one the others take in ([`check_image`]) and within an
/// image's limits (layers, pixels, bytes), found before anything is encoded where it can be.
fn entry(
    cache: &mut HashMap<(u64, u64), Cached>,
    ws: &DocumentWorkspace,
    uid: u64,
) -> Result<Entry, Why> {
    let d = &ws.document;
    let n = d.layers.size();
    if n > MAX_LAYERS {
        return Err(Why::Layers);
    }
    let size = pixels(d.width, d.height, n);
    if size > MAX_PIXELS {
        return Err(Why::Size);
    }
    let list: Vec<Value> = d.layers.iter().map(|l| {
        let p = &l.properties;
        json!({
            "id": l.id, "name": clean_name(&p.name, false), "visible": p.visible, "background": p.is_background,
            "opacity": p.opacity, "blend": p.blend_mode.value(),
            "text": l.text.as_ref().map_or(Value::Null, |t| serde_json::to_value(t).expect("plain data")),
            "tiles": [],
        })
    }).collect();
    let name = ws.file_name.as_deref().map(|n| clean_name(n, true));
    let doc = json!({ "id": uid, "name": name, "width": d.width, "height": d.height, "resolution": d.resolution, "layers": list });
    let count = tile_count(d.width, d.height);
    check_image(&doc, |_| Some(count)).map_err(|_| Why::Format)?;
    let mut tiles = HashMap::new();
    let mut bytes = bytes_of(&doc, std::iter::empty());
    #[cfg(test)]
    tests::ENCODES.set(tests::ENCODES.get() + 1);
    for l in d.layers.iter() {
        let t = tiles_of(cache, uid, l);
        bytes += t.iter().map(|t| t.len() + 4).sum::<usize>();
        if bytes > max_bytes() {
            return Err(Why::Bytes);
        }
        tiles.insert(l.id, t);
    }
    Ok(Entry {
        doc,
        tiles,
        pixels: size,
        bytes,
    })
}

/// The set of `entries` as one, in id order.
fn set<'a>(entries: impl Iterator<Item = &'a Entry>) -> Parts {
    let mut parts = Parts::default();
    let mut docs = Vec::new();
    for e in entries {
        let uid = e.doc["id"].as_u64().unwrap_or_default();
        docs.push(e.doc.clone());
        parts
            .tiles
            .extend(e.tiles.iter().map(|(lid, t)| ((uid, *lid), t.clone())));
    }
    docs.sort_by_key(|d| d["id"].as_u64());
    parts.doc = json!({ "documents": docs });
    parts
}

/// The set's image `uid` from `parts`, as an [`Entry`].
fn entry_of(parts: &Parts, uid: u64, image: &Value) -> Entry {
    let tiles: HashMap<u64, Tiles> = layers(image)
        .filter_map(|(lid, _)| Some((lid, parts.tiles.get(&(uid, lid))?.clone())))
        .collect();
    let (w, h) = size(image).unwrap_or_default();
    let bytes = bytes_of(image, tiles.values());
    Entry {
        doc: image.clone(),
        tiles,
        pixels: pixels(w, h, layers(image).count()),
        bytes,
    }
}

/// What taking in an image does to a layer's pixels: keeps them, patches decoded tiles in (their pixels one after
/// another, and each tile's index with where its pixels start), or makes a new surface.
enum Pixels {
    Keep,
    Patch(Vec<u8>, Vec<(usize, usize)>),
    New(Surface),
}

/// A layer of an image about to be taken in, decoded and allocated already.
struct Planned {
    id: u64,
    properties: LayerProperties,
    text: Option<TextSpec>,
    pixels: Pixels,
    tiles: Tiles,
    hashes: Vec<u64>,
}

/// An image about to be taken in: [`prepare`] did everything that can fail, so [`assemble`] can't.
struct Plan {
    uid: u64,
    size: (u32, u32),
    resolution: f64,
    layers: Vec<Planned>,
}

fn out_of_memory(_: pdn_core::io::ops::OpError) -> String {
    "Not enough memory to take in the shared image.".into()
}

/// Decodes image `uid` of `parts` and reserves its memory, fallibly: one that doesn't fit is an error, not an abort. A
/// layer that `local` has at the same size, whose pixels `cache` holds the tiles of, keeps them and decodes only the
/// tiles that differ; any other layer gets a new surface.
fn prepare(
    parts: &Parts,
    uid: u64,
    image: &Value,
    local: Option<&Document>,
    cache: &HashMap<(u64, u64), Cached>,
) -> Result<Plan, String> {
    let (w, h) = size(image)?;
    let resolution = image["resolution"]
        .as_f64()
        .ok_or_else(|| invalid("resolution"))?;
    let mut out = Vec::new();
    for (lid, l) in layers(image) {
        let (properties, text) = layer_of(l)?;
        let tiles = parts
            .tiles
            .get(&(uid, lid))
            .ok_or_else(|| invalid("tiles missing"))?
            .clone();
        let mine = local
            .and_then(|d| d.layers.iter().find(|x| x.id == lid))
            .filter(|x| (x.surface.width(), x.surface.height()) == (w, h));
        let known = cache.get(&(uid, lid)).filter(|c| {
            mine.is_some_and(|m| c.revision == m.surface.revision())
                && c.size == (w, h)
                && c.tiles.len() == tiles.len()
        });
        let mut hashes = known.map_or_else(|| vec![0; tiles.len()], |c| c.hashes.clone());
        let mut surface = match known {
            Some(_) => None,
            None => Some(pdn_core::io::ops::try_surface(w, h).map_err(out_of_memory)?),
        };
        let differ: Vec<usize> = (0..tiles.len())
            .filter(|&i| known.is_none_or(|c| c.tiles[i] != tiles[i]))
            .collect();
        // Patched into the layer it keeps: the tiles' pixels go in one buffer, reserved fallibly first.
        let (mut patch, mut at) = (Vec::new(), Vec::new());
        if surface.is_none() {
            let rect = |i: &usize| tile_rect(w, h, *i);
            let n = differ
                .iter()
                .map(rect)
                .map(|r| r.2 as usize * r.3 as usize * 4)
                .sum();
            let oom = pdn_core::io::ops::OpError::OutOfMemory;
            patch.try_reserve_exact(n).map_err(|_| out_of_memory(oom))?;
        }
        for i in differ {
            let r = tile_rect(w, h, i);
            let px = decode(&tiles[i], r.2, r.3)?;
            hashes[i] = hash(&px);
            match &mut surface {
                Some(s) => paste(s.data_mut(), w, r, &px),
                None => {
                    at.push((i, patch.len()));
                    patch.extend_from_slice(&px);
                }
            }
        }
        let pixels = match surface {
            Some(s) => Pixels::New(s),
            None if at.is_empty() => Pixels::Keep,
            None => Pixels::Patch(patch, at),
        };
        out.push(Planned {
            id: lid,
            properties,
            text,
            pixels,
            tiles,
            hashes,
        });
    }
    Ok(Plan {
        uid,
        size: (w, h),
        resolution,
        layers: out,
    })
}

/// The image `plan` describes: its layers taken out of `old` (by id) and patched, or new, and what `cache` holds of
/// them then.
fn assemble(
    plan: Plan,
    old: Option<Document>,
    cache: &mut HashMap<(u64, u64), Cached>,
) -> Document {
    let (w, h) = plan.size;
    let mut mine = HashMap::new();
    if let Some(mut old) = old {
        while old.layers.size() > 0 {
            let l = old.layers.remove_layer_at(old.layers.size() - 1);
            mine.insert(l.id, l);
        }
    }
    let mut doc = Document::new(w, h, plan.resolution);
    for p in plan.layers {
        // `prepare` saw the layers it keeps here.
        let mut take = || {
            mine.remove(&p.id)
                .unwrap_or_else(|| Layer::create_layer(w, h, ""))
        };
        let (mut layer, patch, at) = match p.pixels {
            Pixels::New(surface) => {
                let properties = p.properties.clone();
                let layer = Layer {
                    id: p.id,
                    properties,
                    surface,
                    text: None,
                };
                (layer, Vec::new(), Vec::new())
            }
            Pixels::Keep => (take(), Vec::new(), Vec::new()),
            Pixels::Patch(patch, at) => (take(), patch, at),
        };
        if !at.is_empty() {
            let data = layer.surface.data_mut();
            for (i, from) in at {
                let r = tile_rect(w, h, i);
                paste(
                    data,
                    w,
                    r,
                    &patch[from..from + r.2 as usize * r.3 as usize * 4],
                );
            }
        }
        (layer.id, layer.properties, layer.text) = (p.id, p.properties, p.text);
        let cached = Cached {
            revision: layer.surface.revision(),
            size: (w, h),
            hashes: p.hashes,
            tiles: p.tiles,
        };
        cache.insert((plan.uid, p.id), cached);
        doc.add_layer(layer);
    }
    doc
}

// ---------------------------------------------------------------- the session

/// A remote change waiting for a quiet frame: `edited` relative to `base`, or with no base the whole set.
struct Remote {
    base: Option<Vec<u8>>,
    edited: Vec<u8>,
    done: Done,
}

/// Settles a remote change: a whole adopted set gives back the checkpoint it replaced, for the shell's draft.
type Done = Box<dyn FnOnce(Result<Option<Vec<u8>>, String>)>;

type Checkpoint = Result<Rc<Vec<u8>>, String>;

/// An image of the session's set.
struct Shared {
    /// Its id in the set, the same for everyone.
    uid: u64,
    /// The workspace showing it here (`DocumentWorkspace::id`); none once this person closed its tab (the others keep
    /// it) or a change here took it past a limit ([`Why::Left`]).
    local: Option<u64>,
    /// What its entry was made from.
    seen: Option<Sig>,
    entry: Rc<Entry>,
    /// Opened here, and no one else's change to it has come in: closing its tab takes it out of the set.
    mine: bool,
}

/// An image open here that the set doesn't have, and why ([`Why`]). The session never proposes, replaces or closes
/// it. Once it fits, it joins the set as image `uid`; one that left the set's image `uid` ([`Why::Left`]) rejoins only
/// while no one else has changed that since.
struct Alone {
    local: u64,
    uid: u64,
    seen: Option<Sig>,
    why: Why,
    /// For [`Why::Left`]: the set's version of it when it left.
    left: Option<Rc<Entry>>,
    /// A try refused for its bytes (its own, or with the set's): its pixel bytes and the bytes of the set's other images
    /// then. It isn't encoded again until one of those is smaller.
    refused: Option<(u64, usize)>,
}

#[derive(Default)]
struct Live {
    connected: bool,
    /// The last frame was not quiet: a checkpoint now would catch a gesture half done.
    busy: bool,
    docs: Vec<Shared>,
    alone: Vec<Alone>,
    cache: HashMap<(u64, u64), Cached>,
    /// When (app ms) this person's Undo or Redo went to the session: a change taken in within [`FOCUS_MS`] shows its
    /// image, unless this person changed an image since.
    focus: Option<f64>,
    /// The set's checkpoint as last written, and as last reported changed.
    encoded: Option<Checkpoint>,
    reported: Option<Checkpoint>,
    remote: VecDeque<Remote>,
    #[cfg(test)]
    changes: u64,
    #[cfg(test)]
    undos: Vec<bool>,
    ctx: Option<egui::Context>,
    #[cfg(target_arch = "wasm32")]
    hooks: Option<web::Hooks>,
    /// This person's presence as of the last frame, and the selection bounds it was computed from (workspace,
    /// selection revision).
    presence: Value,
    selection: Option<(u64, u64, Value)>,
    /// The others' presence.
    others: Vec<template::Presence>,
    /// Since when (egui time, seconds) others' changes have waited for this person to finish a gesture or a dialog.
    held: Option<f64>,
}

thread_local! {
    static LIVE: std::cell::RefCell<Live> = std::cell::RefCell::default();
}

fn same(a: &Option<Checkpoint>, b: &Option<Checkpoint>) -> bool {
    match (a, b) {
        (Some(Ok(a)), Some(Ok(b))) => Rc::ptr_eq(a, b) || a == b,
        _ => a == b,
    }
}

/// A random 32-bit gap.
fn gap() -> u64 {
    #[cfg(target_arch = "wasm32")]
    return (js_sys::Math::random() * (1u64 << 32) as f64) as u64;
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::hash::BuildHasher as _;
        std::collections::hash_map::RandomState::new().hash_one(0) >> 32
    }
}

/// A new image id for the set: random, so two people opening images at once practically never pick the same one (52
/// bits, exact in JavaScript).
fn uid() -> u64 {
    ((gap() << 20) ^ gap()) & ((1 << 52) - 1)
}

/// Layer ids stay unique across peers while connected: the id counter jumps a random gap past the ids of a set taken
/// in (and at connect), so two people adding layers at once practically never pick the same id.
fn raise_ids(max: u64) {
    if max >= Layer::fresh_id() {
        Layer::ensure_ids_above(max.saturating_add(gap()));
    }
}

/// Whether an image of `pixels` pixel bytes and `bytes` in a checkpoint fits the set beside its other images (all but
/// image `uid`): at most [`MAX_IMAGES`] images, [`MAX_PIXELS`] pixel bytes and [`MAX_BYTES`] (less the header).
fn fits(live: &Live, uid: u64, pixels: u64, bytes: usize) -> Result<(), Why> {
    let (mut n, mut px, mut b) = (1, pixels, bytes);
    for s in live.docs.iter().filter(|s| s.uid != uid) {
        (n, px, b) = (n + 1, px + s.entry.pixels, b + s.entry.bytes);
    }
    if n > MAX_IMAGES || px > MAX_PIXELS || b > max_bytes().saturating_sub(4096) {
        Err(Why::Full)
    } else {
        Ok(())
    }
}

/// [`MAX_BYTES`], which a test can lower to reach a refusal over bytes without encoding 128 MiB.
fn max_bytes() -> usize {
    #[cfg(test)]
    if let Some(n) = tests::MAX_BYTES_HERE.get() {
        return n;
    }
    MAX_BYTES
}

/// The checkpoint bytes of the set's images but `uid`.
fn others_bytes(live: &Live, uid: u64) -> usize {
    (live.docs.iter())
        .filter(|s| s.uid != uid)
        .map(|s| s.entry.bytes)
        .sum()
}

/// A try to share `ws` as the set's image `uid`, refused why, and for a refusal over bytes (found only once encoded)
/// its pixel bytes and the set's other bytes then.
type Tried = Result<Entry, (Why, Option<(u64, usize)>)>;

/// `ws` as the set's image `uid` if it fits: an image's own limits first (layers, pixels: no encoding), then the set's
/// counts and pixels, then its encoded bytes. A try that fails leaves nothing in the cache for an image the set doesn't
/// have.
fn try_entry(live: &mut Live, ws: &DocumentWorkspace, uid: u64) -> Tried {
    let d = &ws.document;
    let n = d.layers.size();
    let px = pixels(d.width, d.height, n);
    let mut encoded = false;
    let why = 'refused: {
        if n > MAX_LAYERS {
            break 'refused Why::Layers;
        }
        if px > MAX_PIXELS {
            break 'refused Why::Size;
        }
        if let Err(why) = fits(live, uid, px, 0) {
            break 'refused why;
        }
        encoded = true;
        let e = match entry(&mut live.cache, ws, uid) {
            Ok(e) => e,
            Err(why) => break 'refused why,
        };
        match fits(live, uid, e.pixels, e.bytes) {
            Ok(()) => return Ok(e),
            Err(why) => why,
        }
    };
    if !live.docs.iter().any(|s| s.uid == uid) {
        live.cache.retain(|k, _| k.0 != uid);
    }
    let refused =
        (encoded && matches!(why, Why::Bytes | Why::Full)).then(|| (px, others_bytes(live, uid)));
    Err((why, refused))
}

/// The set's checkpoint: the images open here that fit it (one opened since joins it) and those closed here as last
/// known. Closing an image this person opened, which no one else changed, takes it out of the set. One that doesn't fit
/// stays open here alone ([`Alone`]); one shared before that a change here took past a limit leaves the set's version
/// as it was.
fn shared(live: &mut Live, app: &AppWorkspace) -> Checkpoint {
    let open = |id: u64| app.documents.iter().any(|ws| ws.id == id);
    let mut changed = live.encoded.is_none();
    live.docs.retain_mut(|s| {
        if s.local.is_none_or(open) {
            return true;
        }
        s.local = None;
        changed |= s.mine;
        !s.mine
    });
    // An image alone closed here goes. One that left the set takes the set's version with it when that is still this
    // person's (no one else changed it).
    let gone: Vec<u64> = (live.alone.iter())
        .filter(|a| !open(a.local) && a.why == Why::Left)
        .map(|a| a.uid)
        .collect();
    live.alone.retain(|a| open(a.local));
    let before = live.docs.len();
    live.docs
        .retain(|s| !(s.mine && s.local.is_none() && gone.contains(&s.uid)));
    changed |= live.docs.len() != before;
    for ws in &app.documents {
        if let Some(i) = live.docs.iter().position(|s| s.local == Some(ws.id)) {
            if unchanged(live.docs[i].seen.as_ref(), ws) {
                continue;
            }
            // A change of this person's: an Undo of theirs that went to the session no longer shows its image.
            live.focus = None;
            let uid = live.docs[i].uid;
            match try_entry(live, ws, uid) {
                Ok(e) => {
                    let s = &mut live.docs[i];
                    (s.entry, s.seen) = (Rc::new(e), Some(sig(ws)));
                    changed = true;
                }
                Err((_, refused)) => {
                    let s = &mut live.docs[i];
                    s.local = None;
                    let left = Some(s.entry.clone());
                    let (seen, why) = (Some(sig(ws)), Why::Left);
                    let local = ws.id;
                    live.alone.push(Alone {
                        local,
                        uid,
                        seen,
                        why,
                        left,
                        refused,
                    });
                }
            }
            continue;
        }
        let a = live.alone.iter().position(|a| a.local == ws.id);
        if let Some(x) = a.map(|a| &live.alone[a]) {
            let d = &ws.document;
            let waits = unchanged(x.seen.as_ref(), ws)
                // One that left rejoins only while the set's version is the one it left.
                || (x.why == Why::Left
                    && !(live.docs.iter()).any(|s| {
                        s.uid == x.uid && x.left.as_ref().is_some_and(|l| Rc::ptr_eq(l, &s.entry))
                    }))
                // One refused for its bytes isn't encoded again until it or the rest of the set is smaller.
                || x.refused.is_some_and(|(px, rest)| {
                    pixels(d.width, d.height, d.layers.size()) >= px && others_bytes(live, x.uid) >= rest
                });
            if waits {
                live.alone[a.unwrap_or_default()].seen = Some(sig(ws));
                continue;
            }
        }
        // A new image, or one alone that changed: it joins the set if it fits.
        let uid = a.map_or_else(uid, |a| live.alone[a].uid);
        match (try_entry(live, ws, uid), a) {
            (Ok(e), a) => {
                if let Some(a) = a {
                    live.alone.remove(a);
                }
                let (seen, entry) = (Some(sig(ws)), Rc::new(e));
                match live.docs.iter_mut().find(|s| s.uid == uid) {
                    // Back from leaving: its content goes in as a change of the set's version (no one else's since).
                    Some(s) => (s.local, s.seen, s.entry) = (Some(ws.id), seen, entry),
                    None => live.docs.push(Shared {
                        uid,
                        local: Some(ws.id),
                        seen,
                        entry,
                        mine: true,
                    }),
                }
                changed = true;
            }
            (Err((why, refused)), Some(a)) => {
                let x = &mut live.alone[a];
                (x.seen, x.refused) = (Some(sig(ws)), refused);
                if x.why != Why::Left {
                    x.why = why;
                }
            }
            (Err((why, refused)), None) => {
                let (local, seen, left) = (ws.id, Some(sig(ws)), None);
                live.alone.push(Alone {
                    local,
                    uid,
                    seen,
                    why,
                    left,
                    refused,
                });
            }
        }
    }
    if changed {
        let set = write(&set(live.docs.iter().map(|s| &*s.entry)));
        live.encoded = Some(set.map(Rc::new));
        prune(live);
    }
    live.encoded.clone().unwrap_or_else(|| Err("busy".into()))
}

/// Only the layers of the set's images stay cached: a layer deleted, an image closed or replaced, leaves nothing behind.
fn prune(live: &mut Live) {
    let keep: HashSet<(u64, u64)> = live
        .docs
        .iter()
        .flat_map(|s| s.entry.tiles.keys().map(move |lid| (s.uid, *lid)))
        .collect();
    live.cache.retain(|key, _| keep.contains(key));
}

/// The set's id of workspace `id`, while connected.
fn uid_of(live: &Live, id: u64) -> Option<u64> {
    live.docs
        .iter()
        .find(|s| s.local == Some(id))
        .map(|s| s.uid)
        .filter(|_| live.connected)
}

pub fn connect() {
    LIVE.with_borrow_mut(|live| {
        // The images keep the ids a checkpoint before connecting gave them: the shell compares with that one.
        live.connected = true;
        live.reported = None;
        if let Some(ctx) = &live.ctx {
            ctx.request_repaint();
        }
    });
    pdn_core::live::set_undo_hook(undo);
    Layer::ensure_ids_above(Layer::fresh_id().saturating_add(gap()));
}

/// Ends the session's hold on Undo; changes still waiting fail. Native Undo goes on from the images as they are: its
/// steps since the last remote change hold no one else's edits.
pub fn disconnect() {
    let waiting = LIVE.with_borrow_mut(|live| {
        live.connected = false;
        (live.docs, live.alone, live.encoded, live.reported) = (Vec::new(), Vec::new(), None, None);
        (live.cache, live.focus) = (HashMap::new(), None);
        (live.presence, live.others, live.selection, live.held) =
            (Value::Null, Vec::new(), None, None);
        #[cfg(target_arch = "wasm32")]
        {
            live.hooks = None;
        }
        if let Some(ctx) = &live.ctx {
            ctx.request_repaint();
        }
        std::mem::take(&mut live.remote)
    });
    for remote in waiting {
        (remote.done)(Err("Live editing disconnected".into()));
    }
}

fn queue(base: Option<Vec<u8>>, edited: Vec<u8>, done: Done) {
    let refused = LIVE.with_borrow_mut(|live| {
        if !live.connected {
            return Some(done);
        }
        live.remote.push_back(Remote { base, edited, done });
        if let Some(ctx) = &live.ctx {
            ctx.request_repaint();
        }
        None
    });
    if let Some(done) = refused {
        done(Err("Live editing is not connected".into()));
    }
}

/// The canonical set of images, unless the last frame was busy: then the next quiet frame reports it.
fn checkpoint(app: &AppWorkspace) -> Checkpoint {
    LIVE.with_borrow_mut(|live| {
        if live.busy {
            live.reported = None;
            return Err("busy".into());
        }
        shared(live, app)
    })
}

/// The Undo hook (`pdn_core::live`): while connected, Undo and Redo of the session's images go to the session. A
/// gesture still open (a shape, text or gradient being edited, a selection being moved) keeps its own steps. Selection
/// steps are personal, no session edits: Undo after one takes back this person's last shared edit and keeps the
/// selection.
fn undo(app: &AppWorkspace, take: Option<bool>) -> bool {
    // Native Undo's own cases: ignored mid-drag (R81), cancelling Perspective Warp or a running fill (R29), stepping an
    // open transaction.
    let Some(ws) = app.active_ws().filter(|_| settled(app)) else {
        return false;
    };
    let taken = LIVE.with_borrow_mut(|live| {
        // An image the set doesn't have, or no set to undo in, keeps native Undo.
        let taken =
            uid_of(live, ws.id).is_some() && live.encoded.as_ref().is_some_and(|e| e.is_ok());
        // The session's Undo is the set's, not the image's: it may take back an edit in another tab. The change it
        // brings is the next one taken in, soon after, which then shows its image.
        if taken && take.is_some() {
            live.focus = Some(app.now_ms);
        }
        #[cfg(test)]
        if let (true, Some(redo)) = (taken, take) {
            live.undos.push(redo);
        }
        taken
    });
    #[cfg(target_arch = "wasm32")]
    if let (true, Some(redo)) = (taken, take) {
        web::call(|hooks| &hooks.undo, &redo.into());
    }
    let _ = take;
    taken
}

/// No gesture under way in the images: no canvas button held, no transaction (a stroke, a shape, text or a gradient
/// being edited, a selection being moved), Move float, parked tool, Perspective Warp, running fill or tool job.
fn settled(app: &AppWorkspace) -> bool {
    !app.canvas_captured()
        && app.perspective.is_none()
        && !app.tool_busy()
        && app.documents.iter().all(|ws| {
            ws.scratch_borrowed.is_none() && ws.float.is_none() && ws.parked_tool.is_none()
        })
}

/// The end of each frame: applies the remote changes waiting and reports a changed shared set, once `quiet` (nothing
/// under way that a remote change would cut short or a checkpoint would catch half done). `pointer`: this person's
/// pointer over the canvas, in image pixels.
pub fn step(app: &mut AppWorkspace, quiet: bool, pointer: Option<[f64; 2]>) {
    let waiting = LIVE.with_borrow_mut(|live| {
        live.busy = !quiet;
        if live.connected && quiet {
            std::mem::take(&mut live.remote)
        } else {
            VecDeque::new()
        }
    });
    for remote in waiting {
        let result = apply(app, remote.base.as_deref(), &remote.edited);
        (remote.done)(result);
    }
    let changed = LIVE.with_borrow_mut(|live| {
        if !live.connected {
            return false;
        }
        // The set first, so an image opened this frame is in it for the presence.
        let now = quiet.then(|| shared(live, app));
        live.presence = presence(live, app, pointer);
        if now.is_none() || same(&now, &live.reported) {
            return false;
        }
        live.reported = now;
        #[cfg(test)]
        {
            live.changes += 1;
        }
        true
    });
    #[cfg(target_arch = "wasm32")]
    if changed {
        web::call(|hooks| &hooks.changed, &wasm_bindgen::JsValue::NULL);
    }
    let _ = changed;
}

/// [`step`] for the running app: nothing pressed or dragged anywhere (a stroke, a slider), no dialog or file chooser,
/// no text field, no transaction, Move float, Perspective Warp, running fill or tool job. Typing on the canvas is a
/// transaction too: the others see the text once it is placed.
#[cfg(any(target_arch = "wasm32", test))]
pub fn frame(app: &mut crate::PaintApp, ctx: &egui::Context) {
    LIVE.with_borrow_mut(|live| {
        live.ctx.get_or_insert_with(|| ctx.clone());
    });
    if !LIVE.with_borrow(|live| live.connected) {
        return step(&mut app.app, true, None);
    }
    let a = &app.app;
    let quiet = !ctx.input(|i| i.pointer.any_down())
        && !ctx.text_edit_focused()
        && settled(a)
        && !app.shell.dialogs.is_open()
        && !crate::platform::files::busy();
    let pointer = ctx.pointer_hover_pos().and_then(|p| {
        let (origin, visible) = app.shell.canvas_view.shown?;
        let ws = a.active_ws().filter(|_| visible.contains(p))?;
        let d = ws.view.to_document_position(
            pdn_core::util::geom::Point::new(f64::from(p.x - origin.x), f64::from(p.y - origin.y)),
            true,
        );
        Some([d.x, d.y])
    });
    step(&mut app.app, quiet, pointer);
    // How long others' changes have waited on this person, for the hint `peers` shows.
    let now = ctx.input(|i| i.time);
    LIVE.with_borrow_mut(|live| {
        let waiting = live.connected && !quiet && !live.remote.is_empty();
        live.held = waiting.then(|| live.held.unwrap_or(now));
    });
}

/// `x` in tenths, short in JSON.
fn num(x: f64) -> Value {
    let x = (x.clamp(-1e7, 1e7) * 10.0).round() / 10.0;
    if x.fract() == 0.0 {
        json!(x as i64)
    } else {
        json!(x)
    }
}

/// About this many characters of numbers at most in a stroke's or lasso's path, so a presence stays well under the
/// shell's 4 KiB; and the most points of a path.
const PATH_CHARS: usize = 2800;
const MAX_POINTS: usize = 200;

/// `points` as `[x, y, …]`: none closer than `min` to the one kept before it (the last always kept), at most the newest
/// [`MAX_POINTS`] and [`PATH_CHARS`].
fn path(points: &[[f64; 2]], min: f64) -> Value {
    let mut kept: Vec<[f64; 2]> = Vec::new();
    for p in points
        .iter()
        .filter(|p| p[0].is_finite() && p[1].is_finite())
    {
        if kept
            .last()
            .is_none_or(|q| (p[0] - q[0]).hypot(p[1] - q[1]) >= min)
        {
            kept.push(*p);
        }
    }
    if let Some(p) = points
        .last()
        .filter(|p| kept.last() != Some(*p) && p[0].is_finite() && p[1].is_finite())
    {
        kept.push(*p);
    }
    let mut chars = 0;
    let newest = kept.iter().rev().take(MAX_POINTS).take_while(|p| {
        chars += num(p[0]).to_string().len() + num(p[1]).to_string().len() + 2;
        chars <= PATH_CHARS
    });
    let from = kept.len() - newest.count();
    kept[from..]
        .iter()
        .flat_map(|p| [num(p[0]), num(p[1])])
        .collect()
}

/// This person's presence: `{ "doc": <the image shown, by its id in the set>, "pointer": [x, y] }` in image pixels
/// while it is over the canvas, `"layers"`: the active layer's id, `"selection": [x0, y0, x1, y1]`, the selection's
/// bounds, when there is one, and the gesture under way: `"stroke": {"size", "points"}`, `"lasso"`, `"band"` (with
/// `"ellipse"`) or `"ghost"` (a Move's selection bounds so far, four corners clockwise). Null while no image of the
/// set is shown.
fn presence(live: &mut Live, app: &AppWorkspace, pointer: Option<[f64; 2]>) -> Value {
    let Some((ws, uid)) = app
        .active_ws()
        .and_then(|ws| Some((ws, uid_of(live, ws.id)?)))
    else {
        return Value::Null;
    };
    let mut state = Map::new();
    state.insert("doc".into(), uid.into());
    if let Some([x, y]) = pointer {
        state.insert("pointer".into(), json!([num(x), num(y)]));
    }
    if let Some(l) = ws.document.layers.get_at(ws.active_layer) {
        state.insert("layers".into(), json!([l.id]));
    }
    // The bounds walk the selection's path: once per selection.
    let revision = ws.selection.revision();
    if live
        .selection
        .as_ref()
        .is_none_or(|(id, rev, _)| (*id, *rev) != (ws.id, revision))
    {
        let b = Some(ws.selection.get_bounds())
            .filter(|b| !ws.selection.is_empty() && b.width > 0.0 && b.height > 0.0);
        let value = b.map_or(Value::Null, |b| {
            json!([num(b.left()), num(b.top()), num(b.right()), num(b.bottom())])
        });
        live.selection = Some((ws.id, revision, value));
    }
    let bounds = live
        .selection
        .as_ref()
        .map(|(.., b)| b.clone())
        .filter(|b| !b.is_null());
    // The drawer's screen point, in image pixels: finer than that shows nothing.
    let min = {
        let a = ws
            .view
            .to_screen_position(pdn_core::util::geom::Point::new(0.0, 0.0));
        let b = ws
            .view
            .to_screen_position(pdn_core::util::geom::Point::new(1.0, 0.0));
        1.0 / (b.x - a.x).abs().max(1e-3)
    };
    use pdn_core::tool::{LiveGesture, ToolType};
    match app.tool.as_ref().and_then(|t| t.live_gesture(app)) {
        Some(LiveGesture::Stroke { size, points }) => {
            state.insert(
                "stroke".into(),
                json!({"size": num(size), "points": path(&points, min)}),
            );
        }
        Some(LiveGesture::Lasso(points)) => {
            state.insert("lasso".into(), path(&points, min));
        }
        Some(LiveGesture::Band { rect, ellipse }) => {
            state.insert("band".into(), rect.iter().map(|x| num(*x)).collect());
            if ellipse {
                state.insert("ellipse".into(), true.into());
            }
        }
        None if matches!(
            app.tool_type,
            Some(ToolType::Move | ToolType::MoveSelection)
        ) && app.canvas_captured() =>
        {
            if let Some(b) = bounds
                .as_ref()
                .and_then(|b| b.as_array())
                .filter(|b| b.len() == 4)
            {
                let [x0, y0, x1, y1] = [&b[0], &b[1], &b[2], &b[3]];
                state.insert("ghost".into(), json!([x0, y0, x1, y0, x1, y1, x0, y1]));
            }
        }
        None => {}
    }
    if let Some(b) = bounds {
        state.insert("selection".into(), b);
    }
    Value::Object(state)
}

/// Merges `edited` (relative to `base`; without one, adopts it whole) into the session's images, with no native undo
/// step ([`install`]).
fn apply(
    app: &mut AppWorkspace,
    base: Option<&[u8]>,
    edited: &[u8],
) -> Result<Option<Vec<u8>>, String> {
    // Nothing is replaced without a checkpoint of it: a merge keeps this person's unsent work, and a whole set gives it
    // back for the shell's draft. The images the set doesn't have (`Alone`) are never touched.
    let current = LIVE.with_borrow_mut(|live| shared(live, app))?;
    if current.as_slice() == edited {
        return Ok(base.is_none().then(|| edited.to_vec()));
    }
    let had = LIVE.with_borrow(|live| !live.docs.is_empty());
    let (bytes, parts) = match base {
        Some(base) => merge(base, &current, edited)?,
        None => (edited.to_vec(), read(edited)?),
    };
    install(app, &parts, base.is_none())?;
    LIVE.with_borrow_mut(|live| {
        live.encoded = Some(Ok(Rc::new(bytes)));
        live.reported = live.encoded.clone();
    });
    // What a whole set replaces goes to the shell's drafts (an empty set is nothing to keep).
    Ok((base.is_none() && had).then(|| current.to_vec()))
}

/// Makes `plan` the image of workspace `ws` as a change made elsewhere: the selection, the view and the active layer (by
/// id, while it exists) stay; the native history, which holds none of it, starts again (undoing a step of it would
/// revert it).
fn install_into(
    app: &mut AppWorkspace,
    ws: usize,
    plan: Plan,
    cache: &mut HashMap<(u64, u64), Cached>,
) {
    let w = &mut app.documents[ws];
    let active = w.document.layers.get_at(w.active_layer).map(|l| l.id);
    let old = w.document.id;
    let mine = std::mem::replace(&mut w.document, Document::new(1, 1, 96.0));
    let doc = assemble(plan, Some(mine), cache);
    let index = doc
        .layers
        .iter()
        .position(|l| Some(l.id) == active)
        .unwrap_or(w.active_layer.min(doc.layers.size() - 1));
    w.set_document_and_active_layer(doc, index);
    fresh_history(w);
    let new = w.document.id;
    // Each image's Clone Stamp anchors and sources follow its document.
    fn rekey<V>(map: &mut HashMap<(u64, pdn_core::tool::ToolType), V>, old: u64, new: u64) {
        let keys: Vec<_> = map.keys().filter(|k| k.0 == old).copied().collect();
        for k in keys {
            if let Some(v) = map.remove(&k) {
                map.insert((new, k.1), v);
            }
        }
    }
    rekey(&mut app.clone_anchors, old, new);
    rekey(&mut app.clone_sources, old, new);
}

fn fresh_history(ws: &mut DocumentWorkspace) {
    ws.history.clear_all();
    ws.history.push_new_memento(HistoryMemento::null(
        &pdn_core::i18n::t(HISTORY.0),
        HISTORY.1,
    ));
    ws.fade_source = None;
    ws.set_dirty(true);
}

/// Opens `plan` as a new tab in the background (shown only when no image is).
fn open(
    app: &mut AppWorkspace,
    plan: Plan,
    name: Option<String>,
    cache: &mut HashMap<(u64, u64), Cached>,
) -> u64 {
    let doc = assemble(plan, None, cache);
    let mut ws = DocumentWorkspace::new(doc.width, doc.height, doc.resolution);
    ws.view.view_width = app.view_width;
    ws.view.view_height = app.view_height;
    ws.view.ppp = app.pixels_per_point;
    ws.view.fit_viewport();
    let top = doc.layers.size() - 1;
    ws.set_document_and_active_layer(doc, top);
    fresh_history(&mut ws);
    ws.file_name = name;
    let id = ws.id;
    app.documents.push(ws);
    app.fire(pdn_core::app::AppEvent::CreateDocument);
    if app.active.is_none() {
        app.set_active_document_workspace(Some(app.documents.len() - 1));
    }
    id
}

/// Makes the session's images those of `set`: each open one takes its new content (keeping this person's views), one
/// someone else opened opens as a tab in the background, one closed here stays closed, and one no longer in the set
/// (another whole set adopted: `whole`) closes; one this person opened that their own Undo takes out of the set stays
/// open here alone, with what they did in it ([`Why::Removed`]). Images the set doesn't have ([`Alone`]) are left as
/// they are. When the image shown closes, the set's first one shows; after this person's Undo or Redo went to the
/// session, the image it changed shows.
fn install(app: &mut AppWorkspace, set: &Parts, whole: bool) -> Result<(), String> {
    let shown = app.active_ws().map(|ws| ws.id);
    raise_ids(order(&set.doc).iter().map(|k| k.1).max().unwrap_or(0));
    // Every image that changed is decoded and its memory reserved first: one that doesn't load changes nothing.
    let mut changed = Vec::new();
    // The composition and scratch surfaces of the workspaces opened or resized, all reserved at once below.
    let mut surfaces = 0u64;
    for (uid, image) in documents(&set.doc) {
        let entry = entry_of(set, uid, image);
        let known = LIVE.with_borrow(|live| {
            let s = live.docs.iter().find(|s| s.uid == uid)?;
            let same = s.entry.doc == entry.doc && s.entry.tiles == entry.tiles;
            Some((s.local, same))
        });
        let load = match known {
            Some((_, true)) => continue,
            Some((None, false)) => None,
            Some((Some(id), false)) => Some(Some(id)),
            None => Some(None),
        };
        let plan = match load {
            Some(target) => {
                let local = target
                    .and_then(|id| app.documents.iter().find(|ws| ws.id == id))
                    .map(|ws| &ws.document);
                let plan = LIVE.with_borrow(|live| prepare(set, uid, image, local, &live.cache))?;
                let (w, h) = plan.size;
                if local.is_none_or(|d| (d.width, d.height) != (w, h)) {
                    surfaces += 2 * pixels(w, h, 1);
                }
                Some((target, plan))
            }
            None => None,
        };
        changed.push((uid, Rc::new(entry), plan));
    }
    if surfaces > 0 {
        let extra = usize::try_from(surfaces)
            .map_err(|_| out_of_memory(pdn_core::io::ops::OpError::TooLarge))?;
        pdn_core::io::ops::check_memory(0, 1, 1, extra).map_err(out_of_memory)?;
    }
    let gone: Vec<u64> = LIVE.with_borrow_mut(|live| {
        let in_set = |s: &Shared| documents(&set.doc).any(|(uid, _)| uid == s.uid);
        let mut gone = Vec::new();
        for s in live.docs.iter().filter(|s| !in_set(s)) {
            match s.local {
                // Only this person's own Undo drops an image they opened while it is theirs (no one else's change
                // came in for it): keeping it open keeps what they did in it, which may not all be in the set yet.
                Some(local) if s.mine && !whole => live.alone.push(Alone {
                    local,
                    uid: s.uid,
                    seen: s.seen.clone(),
                    why: Why::Removed,
                    left: None,
                    refused: None,
                }),
                Some(local) => gone.push(local),
                None => {}
            }
        }
        live.docs.retain(in_set);
        gone
    });
    let mut first_changed = None;
    for (uid, entry, plan) in changed {
        let mut local = None;
        if let Some((target, plan)) = plan {
            let name = entry.doc["name"].as_str().map(str::to_owned);
            let id = LIVE.with_borrow_mut(|live| {
                let cache = &mut live.cache;
                match target.and_then(|id| app.documents.iter().position(|ws| ws.id == id)) {
                    Some(i) => {
                        app.documents[i].file_name = name;
                        install_into(app, i, plan, cache);
                        app.documents[i].id
                    }
                    None => open(app, plan, name, cache),
                }
            });
            local = Some(id);
            first_changed = first_changed.or(local);
        }
        let seen = local
            .and_then(|id| app.documents.iter().find(|ws| ws.id == id))
            .map(sig);
        // Someone else changed it (or opened it): closing it here no longer takes it out of the set.
        LIVE.with_borrow_mut(|live| match live.docs.iter_mut().find(|s| s.uid == uid) {
            Some(s) => {
                s.local = local.or(s.local);
                (s.seen, s.entry, s.mine) = (seen.or(s.seen.take()), entry, false);
            }
            None => live.docs.push(Shared {
                uid,
                local,
                seen,
                entry,
                mine: false,
            }),
        });
    }
    for id in gone {
        if let Some(i) = app.documents.iter().position(|ws| ws.id == id) {
            app.close_document_workspace(i);
        }
    }
    LIVE.with_borrow_mut(prune);
    // No focus stolen: the image shown stays, unless it went.
    let first = LIVE.with_borrow(|live| {
        live.docs
            .iter()
            .filter(|s| s.local.is_some())
            .min_by_key(|s| s.uid)
            .and_then(|s| s.local)
    });
    let undone = LIVE
        .with_borrow_mut(|live| live.focus.take())
        .is_some_and(|at| app.now_ms - at <= FOCUS_MS);
    let show = (first_changed.filter(|_| undone))
        .or(shown.filter(|id| app.documents.iter().any(|ws| ws.id == *id)))
        .or(first);
    if let Some(i) = show.and_then(|id| app.documents.iter().position(|ws| ws.id == id))
        && app.active != Some(i)
    {
        app.set_active_document_workspace(Some(i));
    }
    Ok(())
}

/// The adapter object the Playground shell drives (`shell/live.js`): `format`, `checkpoint()` (throws `busy` while a
/// gesture is under way), `merge(base, current, edited)`, `apply(base | null, edited)` (a promise settled at the next
/// quiet frame; a whole adopted set resolves to the one it replaced), `connect({ changed, undo })`, `disconnect()`,
/// `presence()` and `showPresence(others)`. The hooks are called from inside a frame: the shell defers its calls back.
#[cfg(target_arch = "wasm32")]
pub mod web {
    use super::LIVE;
    use js_sys::{Function, Object, Promise, Reflect, Uint8Array};
    use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};

    pub(super) struct Hooks {
        pub(super) changed: Function,
        pub(super) undo: Function,
    }

    pub(super) fn call(pick: impl FnOnce(&Hooks) -> &Function, arg: &JsValue) {
        if let Some(hook) =
            LIVE.with_borrow(|live| live.hooks.as_ref().map(|hooks| pick(hooks).clone()))
        {
            let _ = hook.call1(&JsValue::NULL, arg);
        }
    }

    /// Whether this page runs inside Playground: its prelude script (which `scripts/fetch.mjs` puts first in every app
    /// page), its shell script (`scripts/live-overlay.mjs`'s test pages) or the shell itself is there.
    fn in_playground() -> bool {
        let page = web_sys::window().and_then(|w| w.document());
        let has = |selector: &str| {
            page.as_ref()
                .is_some_and(|d| d.query_selector(selector).ok().flatten().is_some())
        };
        has("#playground-prelude")
            || has(r#"script[src="/shell/app.js"]"#)
            || Reflect::has(&js_sys::global(), &"playgroundLive".into()).unwrap_or(false)
    }

    /// Offers the adapter as `globalThis.playgroundLiveAdapter`, and to `globalThis.playgroundLive.attach` when the
    /// shell is there already; nothing outside Playground.
    pub fn attach(runner: eframe::WebRunner) -> Result<(), JsValue> {
        if !in_playground() {
            return Ok(());
        }
        let adapter = Object::new();
        let set =
            |name: &str, value: JsValue| Reflect::set(&adapter, &name.into(), &value).map(drop);
        set(
            "format",
            format!("paint-net:1:{}", env!("CARGO_PKG_VERSION")).into(),
        )?;
        let checkpoint = move || -> Result<Uint8Array, JsValue> {
            // Not running yet, or inside a frame: the shell retries a busy checkpoint.
            let bytes = runner
                .app_mut::<crate::PaintApp>()
                .map(|app| super::checkpoint(&app.app))
                .ok_or("busy")?;
            Ok(bytes.map_err(|e| JsValue::from_str(&e))?.as_slice().into())
        };
        set(
            "checkpoint",
            Closure::<dyn Fn() -> _>::new(checkpoint).into_js_value(),
        )?;
        let merge = |base: Uint8Array,
                     current: Uint8Array,
                     edited: Uint8Array|
         -> Result<Uint8Array, JsValue> {
            let (merged, _) = super::merge(&base.to_vec(), &current.to_vec(), &edited.to_vec())
                .map_err(|e| JsValue::from_str(&e))?;
            Ok(merged.as_slice().into())
        };
        set(
            "merge",
            Closure::<dyn Fn(_, _, _) -> _>::new(merge).into_js_value(),
        )?;
        let apply = |base: JsValue, edited: Uint8Array| -> Promise {
            let mut job = Some((
                base.dyn_ref::<Uint8Array>().map(Uint8Array::to_vec),
                edited.to_vec(),
            ));
            Promise::new(&mut |resolve, reject| {
                let Some((base, edited)) = job.take() else {
                    return;
                };
                super::queue(
                    base,
                    edited,
                    Box::new(move |result| {
                        let _ = match result {
                            Ok(None) => resolve.call0(&JsValue::NULL),
                            Ok(Some(replaced)) => resolve
                                .call1(&JsValue::NULL, &Uint8Array::from(replaced.as_slice())),
                            Err(e) => reject.call1(&JsValue::NULL, &JsValue::from_str(&e)),
                        };
                    }),
                );
            })
        };
        set(
            "apply",
            Closure::<dyn Fn(_, _) -> _>::new(apply).into_js_value(),
        )?;
        let connect = |hooks: JsValue| -> Result<(), JsValue> {
            let get = |name: &str| Reflect::get(&hooks, &name.into())?.dyn_into::<Function>();
            let hooks = Hooks {
                changed: get("changed")?,
                undo: get("undo")?,
            };
            LIVE.with_borrow_mut(|live| live.hooks = Some(hooks));
            super::connect();
            Ok(())
        };
        set(
            "connect",
            Closure::<dyn Fn(_) -> _>::new(connect).into_js_value(),
        )?;
        set(
            "disconnect",
            Closure::<dyn Fn()>::new(super::disconnect).into_js_value(),
        )?;
        let presence = || js_sys::JSON::parse(&LIVE.with_borrow(|live| live.presence.to_string()));
        set(
            "presence",
            Closure::<dyn Fn() -> _>::new(presence).into_js_value(),
        )?;
        let show_presence = |others: JsValue| -> Result<(), JsValue> {
            let others: String = js_sys::JSON::stringify(&others)?.into();
            let others =
                super::template::Presence::list(&serde_json::from_str(&others).unwrap_or_default());
            LIVE.with_borrow_mut(|live| {
                live.others = others;
                if let Some(ctx) = &live.ctx {
                    ctx.request_repaint();
                }
            });
            Ok(())
        };
        set(
            "showPresence",
            Closure::<dyn Fn(_) -> _>::new(show_presence).into_js_value(),
        )?;
        let global = js_sys::global();
        Reflect::set(&global, &"playgroundLiveAdapter".into(), &adapter)?;
        let attach = Reflect::get(&global, &"playgroundLive".into())
            .and_then(|live| Reflect::get(&live, &"attach".into()))
            .ok()
            .and_then(|attach| attach.dyn_into::<Function>().ok());
        if let Some(attach) = attach {
            attach.call1(&global, &adapter)?;
        }
        Ok(())
    }
}
