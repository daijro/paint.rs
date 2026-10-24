//! Paint.NET's native `.pdn` container (PDN3 header, NRBF object graph, deferred gzip pixel
//! blocks) and paintdotjs's legacy `paint.js.pdn/1` JSON files. The decode precedence follows
//! Paint.NET, not paintdotjs: a layer's blend mode comes from the class of its legacy `blendOp`
//! object, not from `LayerProperties.blendMode`. The writer generates the NRBF graph itself
//! (paintdotjs patches 64 fixed templates), so each layer's blendOp class matches its mode and
//! there is no layer cap; an all-Normal graph equals paintdotjs's output byte for byte.
//! Ports: paintdotjs js/src/io/DocumentIO.js `readPdn` (the `paint.js.pdn/1` JSON file) and
//! js/src/io/PdnDocumentCodec.js `decode`, `readPixelBlock`, `repairLegacyBlendModeRecords`,
//! `findBlendModeValueOffset`, `encode`, `createHeader`, `encodeSurface` (reference: paintdotjs @
//! b4e6fc5). Where paintdotjs and Paint.NET differ the codec follows Paint.NET.

use std::borrow::Cow;
use std::fmt;

use super::nrbf::{self, NrbfError, NrbfValue};
use crate::canvas::ImageData;
use crate::document::{
    Document,
    layer::{BlendMode, Layer, LayerProperties},
    text_layer::TextSpec,
};

pub const GZIP_MEMBER_HEADER: [u8; 10] = [0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0x00, 0x0a];

/// Per-side limit (paintdotjs `createWorkspaceFromPdnData`).
const MAX_SIDE: i64 = 32768;
// Hostile-input guard: decoded pixel bytes of all layers together. A tiny file can claim a huge
// canvas whose chunks gzip to almost nothing.
use super::codecs::MAX_DECODE_BYTES as MAX_TOTAL_BYTES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdnError(pub String);

impl fmt::Display for PdnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PdnError {}

impl From<NrbfError> for PdnError {
    fn from(e: NrbfError) -> Self {
        PdnError(e.to_string())
    }
}

type Result<T> = std::result::Result<T, PdnError>;

fn err<T>(m: &str) -> Result<T> {
    Err(PdnError(m.to_string()))
}

#[derive(Debug, Clone)]
pub struct PdnDocument {
    pub width: u32,
    pub height: u32,
    /// Dots per inch (X resolution from the EXIF metadata; 96 when absent).
    pub resolution: f64,
    /// Bottom layer first, pixels straight RGBA.
    pub layers: Vec<(LayerProperties, ImageData)>,
    /// Per layer (`layers`' order), the text spec restored from its `paint.rs:text` metadata
    /// (S§4.5); missing entries are raster layers.
    pub texts: Vec<Option<TextSpec>>,
    /// The `<pdnImage>` header XML (custom + thumb), kept so a save can re-write it.
    pub header_xml: String,
}

impl PdnDocument {
    pub fn to_document(&self) -> Document {
        let mut doc = Document::new(self.width, self.height, self.resolution);
        for (i, (props, img)) in self.layers.iter().enumerate() {
            let mut layer = Layer::create_layer(self.width, self.height, &props.name);
            layer.surface.data_mut().copy_from_slice(&img.data);
            layer.properties = props.clone();
            layer.text = self.texts.get(i).cloned().flatten();
            doc.add_layer(layer);
        }
        doc
    }
}

pub fn is_native_pdn(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PDN3")
}

// ---- gzip ---------------------------------------------------------------------------------

pub(crate) fn crc32(data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut t = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
                k += 1;
            }
            t[i] = c;
            i += 1;
        }
        t
    };
    !data.iter().fold(!0u32, |c, &b| {
        TABLE[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8)
    })
}

/// One gzip member in the 5.1.12 form: `GZIP_MEMBER_HEADER`, deflate, CRC32, ISIZE (RFC 1952).
pub fn gzip_member(data: &[u8]) -> Vec<u8> {
    let mut out = GZIP_MEMBER_HEADER.to_vec();
    out.extend(miniz_oxide::deflate::compress_to_vec(data, 6));
    out.extend(crc32(data).to_le_bytes());
    out.extend((data.len() as u32).to_le_bytes());
    out
}

/// Inflates one gzip member (header parsed by hand), refusing to produce more than `limit` bytes,
/// and checks the CRC32 and ISIZE trailer (a missing or wrong trailer is an error).
fn gunzip_limited(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    let bad = || PdnError("bad gzip header".into());
    if data.len() < 18 || data[..3] != [0x1f, 0x8b, 8] {
        return Err(bad());
    }
    let flags = data[3];
    let mut p = 10;
    if flags & 4 != 0 {
        let n = u16::from_le_bytes([data[p], data[p + 1]]) as usize;
        p += 2 + n;
    }
    for bit in [8, 16] {
        if flags & bit != 0 {
            let z = data.get(p..).and_then(|s| s.iter().position(|&b| b == 0));
            p += z.ok_or_else(bad)? + 1;
        }
    }
    if flags & 2 != 0 {
        p += 2;
    }
    let (body, trailer) = data.split_at(data.len() - 8);
    let body = body.get(p..).ok_or_else(bad)?;
    let out = miniz_oxide::inflate::decompress_to_vec_with_limit(body, limit)
        .map_err(|e| PdnError(format!("gzip data: {:?}", e.status)))?;
    let crc = u32::from_le_bytes(trailer[..4].try_into().unwrap());
    let isize = u32::from_le_bytes(trailer[4..].try_into().unwrap());
    if crc != crc32(&out) || isize != out.len() as u32 {
        return err("gzip checksum mismatch");
    }
    Ok(out)
}

fn be32(b: &[u8], at: usize) -> Result<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_be_bytes(s.try_into().unwrap()))
        .ok_or(PdnError("truncated pixel block".into()))
}

/// Reads one deferred MemoryBlock of `length` bytes from the start of `bytes` (Read step 4).
/// Returns the bytes and how many input bytes the block occupied.
pub fn read_deferred_block(bytes: &[u8], length: usize) -> Result<(Vec<u8>, usize)> {
    if length as u64 > MAX_TOTAL_BYTES {
        return err("pixel block too large");
    }
    let format = *bytes
        .first()
        .ok_or(PdnError("truncated pixel block".into()))?;
    if format > 1 {
        return err("unknown pixel block format");
    }
    let chunk_size = be32(bytes, 1)? as usize;
    if chunk_size == 0 {
        return err("zero chunk size");
    }
    let count = length.div_ceil(chunk_size);
    // Every chunk needs an 8-byte header, so a count the input cannot hold is refused before
    // anything is allocated.
    if count > (bytes.len() - 5) / 8 {
        return err("truncated pixel block");
    }
    let mut out = Vec::new();
    out.try_reserve_exact(length)
        .map_err(|_| PdnError("out of memory".into()))?;
    out.resize(length, 0);
    let mut seen = vec![false; count];
    let mut p = 5;
    for _ in 0..count {
        let idx = be32(bytes, p)? as usize;
        let n = be32(bytes, p + 4)? as usize;
        p += 8;
        let data = p
            .checked_add(n)
            .and_then(|e| bytes.get(p..e))
            .ok_or(PdnError("truncated pixel chunk".into()))?;
        p += n;
        if idx >= count || std::mem::replace(&mut seen[idx], true) {
            return err("bad pixel chunk index");
        }
        let start = idx * chunk_size;
        let slot = &mut out[start..(start + chunk_size).min(length)];
        let raw = if format == 0 {
            gunzip_limited(data, slot.len())?
        } else {
            data.to_vec()
        };
        if raw.len() > slot.len() {
            return err("pixel chunk too long");
        }
        slot[..raw.len()].copy_from_slice(&raw);
    }
    Ok((out, p))
}

// ---- NRBF graph -> document ---------------------------------------------------------------

/// The mode named by the `PaintDotNet.UserBlendOps+<Mode>BlendOp` class name a .pdn stores.
fn blend_mode_of_class(class: &str) -> Option<BlendMode> {
    let name = class.rsplit(['+', '.']).next()?.strip_suffix("BlendOp")?;
    BlendMode::ALL
        .into_iter()
        .find(|m| format!("{m:?}") == name)
}

fn dim(v: &NrbfValue) -> Result<u32> {
    match v.as_i64() {
        Some(n) if (1..=MAX_SIDE).contains(&n) => Ok(n as u32),
        _ => err("invalid canvas size"),
    }
}

fn properties(layer: &NrbfValue, index: usize) -> LayerProperties {
    let p = layer.get("Layer+properties");
    // A name string, even an empty one, is the layer's name (Paint.NET keeps it); only a missing
    // name gets paintdotjs's "Layer N".
    let name = p
        .get("name")
        .as_str()
        .map_or_else(|| format!("Layer {}", index + 1), str::to_string);
    let mut lp = LayerProperties::new(
        &name,
        p.get("visible").as_bool().unwrap_or(true),
        p.get("isBackground").as_bool().unwrap_or(false),
        p.get("opacity")
            .as_i64()
            .map_or(255, |o| o.clamp(0, 255) as u8),
    );
    let bm = p.get("blendMode");
    lp.blend_mode = bm
        .get("value__")
        .as_i64()
        .or(bm.as_i64())
        .map_or(BlendMode::Normal, |v| {
            BlendMode::from_pdn_value(v.clamp(0, 255) as u8)
        });
    // §2.11 Read: on load, the legacy opacity (when set) and the blendOp class win.
    let legacy = layer.get("properties");
    if let Some(o) = legacy.get("opacity").as_i64().filter(|&o| o != -1) {
        lp.opacity = o.clamp(0, 255) as u8;
    }
    let op = legacy.get("blendOp");
    if !op.is_null() {
        lp.blend_mode = op
            .class()
            .and_then(blend_mode_of_class)
            .unwrap_or(BlendMode::Normal);
    }
    lp
}

/// The `userMetadataItems` key of a text layer's spec (S§4.5).
const TEXT_KEY: &str = "paint.rs:text";

/// The `paint.rs:text` value: the spec and the hash of the pixels it was saved with.
#[derive(serde::Serialize, serde::Deserialize)]
struct TextMeta {
    v: u32,
    spec: TextSpec,
    hash: String,
}

/// 64-bit FNV-1a over a layer surface's stored bytes in storage order: straight RGBA8, rows top
/// down (not the file's BGRA). Ruling R2: only ever compared with paint.rs's own writes.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The layer's text spec (S§4.5), only while the saved hash matches the loaded pixels: a layer
/// edited elsewhere (Paint.NET keeps the metadata) is a plain bitmap. Never re-rendered. Other
/// keys are ignored, and a malformed value leaves a bitmap layer.
fn text_spec(layer: &NrbfValue, rgba: &[u8]) -> Option<TextSpec> {
    let items = layer
        .get("Layer+properties")
        .get("userMetadataItems")
        .as_array()?;
    let value = items
        .iter()
        .find(|kv| kv.get("key").as_str() == Some(TEXT_KEY))?
        .get("value")
        .as_str()?;
    let meta: TextMeta = serde_json::from_str(value).ok()?;
    (meta.v == 1 && u64::from_str_radix(&meta.hash, 16) == Ok(fnv1a(rgba))).then_some(meta.spec)
}

/// An inline MemoryBlock: `pointerData`, or `chunkCount` + `chunkFormat` (0) + raw `chunk<i>`
/// byte arrays whose lengths sum to `length` (Read step 3).
fn inline_block(mb: &NrbfValue, length: usize) -> Result<Cow<'_, [u8]>> {
    let not_bytes = || PdnError("expected a byte array".into());
    if !mb.get("pointerData").is_null() {
        return Ok(Cow::Borrowed(
            mb.get("pointerData").as_bytes().ok_or_else(not_bytes)?,
        ));
    }
    let count = mb
        .get("chunkCount")
        .as_i64()
        .filter(|&c| c >= 0)
        .ok_or(PdnError("MemoryBlock has no data".into()))?;
    if mb.get("chunkFormat").as_i64().unwrap_or(0) != 0 {
        return err("unsupported chunk format");
    }
    let mut out = Vec::new();
    for i in 0..count {
        let chunk = mb
            .get(&format!("chunk{i}"))
            .as_bytes()
            .ok_or_else(not_bytes)?;
        if chunk.len() > length - out.len() {
            return err("MemoryBlock chunks exceed its length");
        }
        out.extend_from_slice(chunk);
    }
    if out.len() != length {
        return err("MemoryBlock chunk lengths do not sum to its length");
    }
    Ok(Cow::Owned(out))
}

// ---- early-paintdotjs blend-mode repair ----------------------------------------------------

/// `layers="N"` from the header XML, when 1..=64 (the template range).
fn layer_count_of(xml: &str) -> Option<usize> {
    xml_attr(xml, "layers")?
        .parse()
        .ok()
        .filter(|n| (1..=64).contains(n))
}

/// Skips a length-prefixed string at `at`; returns the offset after it.
fn skip_string(b: &[u8], at: usize) -> Option<usize> {
    let mut len = 0usize;
    let mut p = at;
    for i in 0..5 {
        let v = *b.get(p)?;
        p += 1;
        if i == 4 && v > 0x07 {
            return None;
        }
        len |= ((v & 0x7f) as usize) << (7 * i);
        if v & 0x80 == 0 {
            let end = p.checked_add(len)?;
            return (end <= b.len()).then_some(end);
        }
    }
    None
}

/// Where the Int32 `value__` of the blend-mode enum record starting at `rec` lives
/// (`findBlendModeValueOffset`): a ClassWithId keeps it at +9; a ClassWithMembersAndTypes holds
/// exactly one member, so it follows the class metadata and library id.
fn blend_value_offset(b: &[u8], rec: usize) -> Option<usize> {
    match *b.get(rec)? {
        1 => Some(rec + 9),
        5 => {
            let mut p = skip_string(b, rec + 5)?;
            let count = i32::from_le_bytes(b.get(p..p + 4)?.try_into().ok()?);
            if count != 1 {
                return None;
            }
            p = skip_string(b, p + 4)?;
            let kind = *b.get(p)?;
            p += 1;
            p = match kind {
                0 | 7 => p + 1,
                3 => skip_string(b, p)?,
                4 => skip_string(b, p)? + 4,
                _ => p,
            };
            Some(p + 4)
        }
        _ => None,
    }
}

/// The 5-byte `BinaryObjectString` signature (type 6 + object id) of layer `i`'s name in the
/// Paint.NET 5.1.12 template for `n` layers, and the 4 record-header bytes (`tag`, low three bytes
/// of the blend enum's object id) paintdotjs overwrote with the blend mode. The templates number
/// objects regularly: name id = 24 + 4(n-1) + 5i, enum id = -(name id + 2), tag 5 for the first
/// layer and 1 (ClassWithId) after. Pinned against all 64 templates by a test.
fn template_layer_bytes(n: usize, i: usize) -> ([u8; 5], [u8; 4]) {
    let name_id = (24 + 4 * (n - 1) + 5 * i) as i32;
    let mut sig = [6, 0, 0, 0, 0];
    sig[1..].copy_from_slice(&name_id.to_le_bytes());
    let enum_id = (-(name_id + 2)).to_le_bytes();
    (
        sig,
        [
            if i == 0 { 5 } else { 1 },
            enum_id[0],
            enum_id[1],
            enum_id[2],
        ],
    )
}

/// Offset of layer `i`'s damaged record header in `graph` (`name string`, +5 reference, +3 flags).
fn legacy_record_offset(graph: &[u8], n: usize, i: usize) -> Option<usize> {
    let (sig, _) = template_layer_bytes(n, i);
    let at = graph.windows(5).position(|w| w == sig)?;
    Some(skip_string(graph, at + 5)? + 5 + 3)
}

/// Port of `repairLegacyBlendModeRecords`: puts the template's record header bytes back and moves
/// the blend mode (found in the damaged header) into the enum's `value__`.
fn repair_legacy_blend_records(graph: &[u8], n: usize) -> Option<Vec<u8>> {
    let mut out = graph.to_vec();
    for i in 0..n {
        let rec = legacy_record_offset(&out, n, i)?;
        let mode = i32::from_le_bytes(out.get(rec..rec + 4)?.try_into().ok()?);
        if !(0..=13).contains(&mode) {
            return None;
        }
        out[rec..rec + 4].copy_from_slice(&template_layer_bytes(n, i).1);
        let at = blend_value_offset(&out, rec)?;
        out.get_mut(at..at + 4)?
            .copy_from_slice(&mode.to_le_bytes());
    }
    Some(out)
}

/// Parses the NRBF graph; on failure, tries the early-paintdotjs blend-record repair. Early
/// paintdotjs builds wrote the blend mode over four bytes of an NRBF record header; the repair
/// handles exactly that layout (`layers="N"` from the header XML) and otherwise the original error
/// is reported. Returns the root and the bytes consumed.
fn parse_graph(after: &[u8], header_xml: &str) -> Result<(NrbfValue, usize)> {
    match nrbf::deserialize(after) {
        Ok(r) => Ok(r),
        Err(e) => {
            let repaired = layer_count_of(header_xml)
                .and_then(|n| repair_legacy_blend_records(after, n))
                .ok_or(e)?;
            Ok(nrbf::deserialize(&repaired)?)
        }
    }
}

fn read_native(bytes: &[u8]) -> Result<PdnDocument> {
    // Step 1: header (or none: a legacy stream).
    let (header_xml, marker) = if bytes.starts_with(b"PDN") {
        if bytes.get(3) != Some(&b'3') {
            return err("unsupported .pdn version");
        }
        if bytes.len() < 7 {
            return err("truncated .pdn header");
        }
        let xml_len = bytes[4] as usize | (bytes[5] as usize) << 8 | (bytes[6] as usize) << 16;
        let xml = bytes
            .get(7..7 + xml_len)
            .ok_or(PdnError("truncated .pdn header".into()))?;
        (String::from_utf8_lossy(xml).into_owned(), 7 + xml_len)
    } else {
        (String::new(), 0)
    };
    // Step 2: format marker.
    let after = bytes
        .get(marker + 2..)
        .ok_or(PdnError("truncated .pdn".into()))?;
    let (root, mut cursor, deferred): (NrbfValue, usize, Option<&[u8]>) =
        match bytes[marker..marker + 2] {
            [0, 1] => {
                let (root, used) = parse_graph(after, &header_xml)?;
                (root, 0, Some(&after[used..]))
            }
            [0x1f, 0x8b] => {
                let raw = gunzip_limited(&bytes[marker..], MAX_TOTAL_BYTES as usize)?;
                (nrbf::deserialize_sized(&raw, bytes.len())?.0, 0, None)
            }
            _ => return err("unsupported .pdn encoding"),
        };

    let (width, height) = (dim(root.get("width"))?, dim(root.get("height"))?);
    let list = root.get("layers");
    let items = if list.get("ArrayList+_items").is_null() {
        list.get("_items")
    } else {
        list.get("ArrayList+_items")
    };
    let size = if list.get("ArrayList+_size").is_null() {
        list.get("_size")
    } else {
        list.get("ArrayList+_size")
    };
    let size = size
        .as_i64()
        .filter(|&s| s >= 1)
        .and_then(|s| usize::try_from(s).ok())
        .ok_or(PdnError("document has no layers".into()))?;
    let items = items
        .as_array()
        .filter(|a| a.len() >= size)
        .ok_or(PdnError("invalid layer list".into()))?;

    let row = width as usize * 4;
    let mut total = 0u64;
    let mut layers = Vec::new();
    let mut texts = Vec::new();
    for (i, layer) in items[..size].iter().enumerate() {
        if layer.is_null() {
            return err("invalid layer entry");
        }
        let surface = layer.get("surface");
        if dim(surface.get("width"))? != width || dim(surface.get("height"))? != height {
            return err("layer size differs from the document");
        }
        let stride = usize::try_from(surface.get("stride").as_i64().unwrap_or(0))
            .ok()
            .filter(|&s| s >= row && s <= i32::MAX as usize)
            .ok_or(PdnError("invalid surface stride".into()))?;
        let mb = surface.get("scan0");
        let length = mb
            .get("length64")
            .as_i64()
            .or(mb.get("length").as_i64())
            .unwrap_or(-1);
        let need = stride as u64 * height as u64;
        if length < 0 || (length as u64) < need {
            return err("surface data shorter than its dimensions");
        }
        total += length as u64;
        if total > MAX_TOTAL_BYTES {
            return err("document too large");
        }
        // Within MAX_TOTAL_BYTES, so it fits usize on wasm32 too.
        let length = usize::try_from(length).map_err(|_| PdnError("document too large".into()))?;
        let data: Cow<[u8]> = if mb.get("deferred").as_bool().unwrap_or(false) {
            let rest = deferred.ok_or(PdnError("deferred block in a gzip stream".into()))?;
            let (d, used) = read_deferred_block(rest.get(cursor..).unwrap_or(&[]), length)?;
            cursor += used;
            Cow::Owned(d)
        } else {
            inline_block(mb, length)?
        };
        if data.len() < need as usize {
            return err("surface data shorter than its dimensions");
        }
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(row * height as usize)
            .map_err(|_| PdnError("out of memory".into()))?;
        for y in 0..height as usize {
            for px in data[y * stride..y * stride + row].as_chunks::<4>().0 {
                rgba.extend([px[2], px[1], px[0], px[3]]);
            }
        }
        texts.push(text_spec(layer, &rgba));
        layers.push((
            properties(layer, i),
            ImageData {
                width,
                height,
                data: rgba,
            },
        ));
    }
    Ok(PdnDocument {
        width,
        height,
        resolution: resolution_of(&root),
        layers,
        texts,
        header_xml,
    })
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn xml_attr<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let at = xml.find(&format!("{name}=\""))? + name.len() + 2;
    xml[at..].split('"').next()
}

/// X resolution (EXIF 282, a rational) and unit (296; 3 = per centimetre) from the document's
/// `$exif.*` metadata items; 96 dpi when absent or unusable.
fn resolution_of(root: &NrbfValue) -> f64 {
    let mut dpi = None;
    let mut cm = false;
    for item in root.get("userMetadataItems").as_array().unwrap_or(&[]) {
        let Some(xml) = item.get("value").as_str() else {
            continue;
        };
        let Some(data) = xml_attr(xml, "value").and_then(base64_decode) else {
            continue;
        };
        match xml_attr(xml, "id") {
            Some("282") if data.len() >= 8 => {
                let n = u32::from_le_bytes(data[..4].try_into().unwrap()) as f64;
                let d = u32::from_le_bytes(data[4..8].try_into().unwrap()) as f64;
                dpi = Some(n / d);
            }
            Some("296") => cm = data.first() == Some(&3),
            _ => {}
        }
    }
    match dpi {
        Some(v) if v.is_finite() && v > 0.0 => {
            if cm {
                v * 2.54
            } else {
                v
            }
        }
        _ => 96.0,
    }
}

// ---- paintdotjs legacy JSON ---------------------------------------------------------------

fn read_json(bytes: &[u8]) -> Result<PdnDocument> {
    let bad = || PdnError("unsupported .pdn document format".into());
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| bad())?;
    if v["format"] != "paint.js.pdn/1" {
        return Err(bad());
    }
    let side = |k: &str| match v[k].as_i64() {
        Some(n) if (1..=MAX_SIDE).contains(&n) => Ok(n as u32),
        _ => err("invalid .pdn canvas size"),
    };
    let (width, height) = (side("width")?, side("height")?);
    if width as u64 * height as u64 * 4 > MAX_TOTAL_BYTES {
        return err("document too large");
    }
    let resolution = v["resolution"]
        .as_f64()
        .filter(|r| r.is_finite() && *r > 0.0)
        .unwrap_or(96.0)
        .max(0.01);
    let mut layers = Vec::new();
    let mut total = 0u64;
    for (i, saved) in v["layers"].as_array().ok_or_else(bad)?.iter().enumerate() {
        total += width as u64 * height as u64 * 4;
        if total > MAX_TOTAL_BYTES {
            return err("document too large");
        }
        let name = saved["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map_or(format!("Layer {}", i + 1), String::from);
        let mut lp = LayerProperties::new(
            &name,
            saved["visible"].as_bool().unwrap_or(true),
            saved["isBackground"].as_bool().unwrap_or(false),
            saved["opacity"]
                .as_f64()
                .map_or(255, |o| o.round().clamp(0.0, 255.0) as u8),
        );
        // LayerProperties.getBlendMode: an unknown or missing mode name falls back to Normal.
        lp.blend_mode = BlendMode::from_value(saved["blendMode"].as_str().unwrap_or(""));
        let png = saved["png"]
            .as_str()
            .and_then(|u| u.split_once("base64,"))
            .and_then(|(_, b)| base64_decode(b))
            .ok_or(PdnError("layer has no PNG data".into()))?;
        let (pw, ph) = match png.get(16..24) {
            Some(d) => (be32(d, 0)? as u64, be32(d, 4)? as u64),
            None => return err("bad layer PNG"),
        };
        if pw
            .checked_mul(ph)
            .and_then(|n| n.checked_mul(4))
            .is_none_or(|n| n > MAX_TOTAL_BYTES)
        {
            return err("layer PNG too large");
        }
        let pm =
            tiny_skia::Pixmap::decode_png(&png).map_err(|e| PdnError(format!("layer PNG: {e}")))?;
        // drawImage(image, 0, 0): clipped to the canvas, the rest transparent.
        let mut data = vec![0u8; width as usize * height as usize * 4];
        for y in 0..pm.height().min(height) as usize {
            for x in 0..pm.width().min(width) as usize {
                let c = pm.pixels()[y * pm.width() as usize + x].demultiply();
                let o = (y * width as usize + x) * 4;
                data[o..o + 4].copy_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
            }
        }
        layers.push((
            lp,
            ImageData {
                width,
                height,
                data,
            },
        ));
    }
    if layers.is_empty() {
        return err("document has no layers");
    }
    Ok(PdnDocument {
        width,
        height,
        resolution,
        layers,
        texts: Vec::new(),
        header_xml: String::new(),
    })
}

/// Decodes a native `.pdn` (any form Paint.NET accepts) or a paintdotjs `paint.js.pdn/1` file.
pub fn decode(bytes: &[u8]) -> Result<PdnDocument> {
    if bytes.starts_with(b"PDN") || bytes.starts_with(&[0, 1]) || bytes.starts_with(&[0x1f, 0x8b]) {
        read_native(bytes)
    } else {
        read_json(bytes)
    }
}

/// Offset in `bytes` of the first deferred pixel block: the end of the NRBF graph. `None` unless
/// this is a `00 01` file. Early-paintdotjs files are repaired first, as `decode` does, so the
/// offset is the same one `decode` reads the blocks from.
pub fn graph_end(bytes: &[u8]) -> Option<usize> {
    let (xml, marker) = if bytes.starts_with(b"PDN3") {
        let n = *bytes.get(4)? as usize
            | (*bytes.get(5)? as usize) << 8
            | (*bytes.get(6)? as usize) << 16;
        let xml = bytes.get(7..7 + n)?;
        (String::from_utf8_lossy(xml).into_owned(), 7 + n)
    } else {
        (String::new(), 0)
    };
    if bytes.get(marker..marker + 2)? != [0, 1] {
        return None;
    }
    let (_, used) = parse_graph(&bytes[marker + 2..], &xml).ok()?;
    Some(marker + 2 + used)
}

// ---- writer -------------------------------------------------------------------------------

fn base64_encode(data: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            s.push(if i <= c.len() {
                A[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    s
}

/// Write step 3: the thumbnail size, the max-edge rule at 256 (§2.11 thumbnail note, analyst
/// resolution; the same rule as `ui-drawn-graphics.md` §2.8's `thumbnail_size`): the long side
/// becomes min(side, 256) and the short side max(1, short·long′/long), truncated. 300×301 →
/// 255×256; a document within 256×256 is unchanged. Appendix B B-F10 calibrates it.
pub fn thumb_size(w: u32, h: u32) -> (u32, u32) {
    crate::render::thumbnail::thumbnail_size(w, h, 256)
}

// The library names a Paint.NET 5.1.12 .pdn stream stores: file-format data, written as is.
const DATA_LIB: &str =
    "PaintDotNet.Data, Version=5.112.9563.32325, Culture=neutral, PublicKeyToken=null";
const CORE_LIB: &str =
    "PaintDotNet.Core, Version=5.112.9563.32325, Culture=neutral, PublicKeyToken=null";
const KV: &str = "System.Collections.Generic.KeyValuePair`2[[System.String, mscorlib, \
    Version=4.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089],[System.String, mscorlib, \
    Version=4.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089]]";

/// The document's EXIF items (Ruling (P3) 8: exactly the templates' three, no Software tag):
/// ResolutionUnit 2 (inch) and X/YResolution = the DPI as a rational, keyed as the templates.
fn exif_items(dpi: f64) -> [(&'static str, String); 3] {
    // ponytail: integral DPI is n/1 (the templates' 96/1); others are n/10000, so 4 decimals.
    let (n, d) = if dpi.fract() == 0.0 {
        (dpi as u32, 1u32)
    } else {
        ((dpi * 10000.0).round() as u32, 10000)
    };
    let mut r = n.to_le_bytes().to_vec();
    r.extend(d.to_le_bytes());
    let res = |id| {
        format!(
            r#"<exif id="{id}" len="8" type="5" value="{}" />"#,
            base64_encode(&r)
        )
    };
    [
        (
            "$exif.tag1[0]",
            r#"<exif id="296" len="2" type="3" value="AgA=" />"#.into(),
        ),
        ("$exif.tag3[0]", res(282)),
        ("$exif.tag5[0]", res(283)),
    ]
}

/// Write step 5: the NRBF graph of a `w`×`h` document, record for record what BinaryFormatter
/// writes for Paint.NET 5.1.12 (the 64 templates pin it). Object ids follow the formatter's
/// counter, which every reference lookup advances: for N layers, layer i is 17 + i, the Core
/// library 17 + N, layer i's BitmapLayerProperties / Surface / LayerProperties 18 + N + 3i ..,
/// and its blendOp / MemoryBlock / name / (shared metadata array) / blend-mode enum
/// 18 + 4N + 5i .. (the enum, a value type, negative). `texts[i]` is layer i's `paint.rs:text`
/// value (S§4.5; a missing entry is a raster layer): that layer gets its own one-entry metadata
/// array with ids past all of those, so a document without text keeps the template bytes.
fn graph(
    w: u32,
    h: u32,
    dpi: f64,
    layers: &[&LayerProperties],
    texts: &[Option<String>],
) -> Vec<u8> {
    use nrbf::Member::{Class, ObjectArray, Primitive, String as Str, SystemClass};
    use nrbf::{BOOLEAN, BYTE, INT32, INT64};
    let n = layers.len() as i32;
    let (w, h) = (w as i32, h as i32);
    let core = 17 + n;
    let kv_array = format!("{KV}[]");
    let mut g = nrbf::Writer::new(1);
    g.library(2, DATA_LIB);
    g.class(
        1,
        "PaintDotNet.Document",
        &[
            ("isDisposed", Primitive(BOOLEAN)),
            ("layers", Class("PaintDotNet.LayerList", 2)),
            ("width", Primitive(INT32)),
            ("height", Primitive(INT32)),
            ("savedWith", SystemClass("System.Version")),
            ("userMetadataItems", SystemClass(&kv_array)),
        ],
        Some(2),
    );
    g.bool(false);
    g.reference(3);
    g.i32(w);
    g.i32(h);
    g.reference(4);
    g.reference(5);
    g.class(
        3,
        "PaintDotNet.LayerList",
        &[
            ("parent", Class("PaintDotNet.Document", 2)),
            ("ArrayList+_items", ObjectArray),
            ("ArrayList+_size", Primitive(INT32)),
            ("ArrayList+_version", Primitive(INT32)),
        ],
        Some(2),
    );
    g.reference(1);
    g.reference(7);
    g.i32(n);
    g.i32(n);
    g.class(
        4,
        "System.Version",
        &[
            ("_Major", Primitive(INT32)),
            ("_Minor", Primitive(INT32)),
            ("_Build", Primitive(INT32)),
            ("_Revision", Primitive(INT32)),
        ],
        None,
    );
    // Ruling R41: savedWith is the app version, as real Paint.NET writes it (BB: icon-lemon.pdn);
    // paintdotjs's templates hold 0.0.0.0 here.
    for v in [5, 112, 9563, 32325] {
        g.i32(v);
    }
    g.array(5, 3, KV);
    for (k, (key, value)) in (0..).zip(exif_items(dpi)) {
        g.class(-(8 + 3 * k), KV, &[("key", Str), ("value", Str)], None);
        g.string(9 + 3 * k, key);
        g.string(10 + 3 * k, &value);
    }
    // ArrayList capacity: 4, doubling.
    let capacity = layers.len().max(4).next_power_of_two();
    g.object_array(7, capacity);
    for i in 0..n {
        g.reference(17 + i);
    }
    g.nulls(capacity - layers.len());
    g.library(core, CORE_LIB);
    for i in 0..n {
        g.class(
            17 + i,
            "PaintDotNet.BitmapLayer",
            &[
                (
                    "properties",
                    Class("PaintDotNet.BitmapLayer+BitmapLayerProperties", 2),
                ),
                ("surface", Class("PaintDotNet.Surface", core)),
                ("Layer+isDisposed", Primitive(BOOLEAN)),
                ("Layer+width", Primitive(INT32)),
                ("Layer+height", Primitive(INT32)),
                (
                    "Layer+properties",
                    Class("PaintDotNet.Layer+LayerProperties", 2),
                ),
            ],
            Some(2),
        );
        g.reference(18 + n + 3 * i);
        g.reference(19 + n + 3 * i);
        g.bool(false);
        g.i32(w);
        g.i32(h);
        g.reference(20 + n + 3 * i);
    }
    let op_class =
        |p: &LayerProperties| format!("PaintDotNet.UserBlendOps+{:?}BlendOp", p.blend_mode);
    let metadata_items = 21 + 4 * n;
    // A text layer's array, its KeyValuePair (a value type, negative), key and value: 4 ids each
    // from 18 + 9N, past the last layer's enum.
    let mut next = 18 + 9 * n;
    let own: Vec<Option<(i32, &str)>> = (0..layers.len())
        .map(|i| {
            let json = texts.get(i)?.as_deref()?;
            next += 4;
            Some((next - 4, json))
        })
        .collect();
    for (i, p) in (0..).zip(layers) {
        let id = 18 + 4 * n + 5 * i;
        // §2.11 step 5: blendOp always matches the blend mode (Paint.NET loads the mode from it).
        g.class(
            18 + n + 3 * i,
            "PaintDotNet.BitmapLayer+BitmapLayerProperties",
            &[("blendOp", Class(&op_class(p), 2))],
            Some(2),
        );
        g.reference(id);
        g.class(
            19 + n + 3 * i,
            "PaintDotNet.Surface",
            &[
                ("width", Primitive(INT32)),
                ("height", Primitive(INT32)),
                ("stride", Primitive(INT32)),
                ("scan0", Class("PaintDotNet.MemoryBlock", core)),
            ],
            Some(core),
        );
        g.i32(w);
        g.i32(h);
        g.i32(w * 4);
        g.reference(id + 1);
        g.class(
            20 + n + 3 * i,
            "PaintDotNet.Layer+LayerProperties",
            &[
                ("name", Str),
                ("userMetadataItems", SystemClass(&kv_array)),
                ("visible", Primitive(BOOLEAN)),
                ("isBackground", Primitive(BOOLEAN)),
                ("opacity", Primitive(BYTE)),
                ("blendMode", Class("PaintDotNet.LayerBlendMode", 2)),
            ],
            Some(2),
        );
        g.string(id + 2, &p.name);
        g.reference(own[i as usize].map_or(metadata_items, |(a, _)| a));
        g.bool(p.visible);
        g.bool(p.is_background);
        g.u8(p.opacity);
        g.class(
            -(id + 4),
            "PaintDotNet.LayerBlendMode",
            &[("value__", Primitive(INT32))],
            Some(2),
        );
        g.i32(p.blend_mode.pdn_value() as i32);
    }
    let mut shared = false;
    for (i, p) in (0..).zip(layers) {
        let id = 18 + 4 * n + 5 * i;
        g.class(id, &op_class(p), &[], Some(2));
        g.class(
            id + 1,
            "PaintDotNet.MemoryBlock",
            &[
                ("length64", Primitive(INT64)),
                ("hasParent", Primitive(BOOLEAN)),
                ("deferred", Primitive(BOOLEAN)),
            ],
            Some(core),
        );
        g.i64(w as i64 * h as i64 * 4);
        g.bool(false);
        g.bool(true);
        match own[i as usize] {
            Some((a, json)) => {
                g.array(a, 1, KV);
                g.class(-(a + 1), KV, &[("key", Str), ("value", Str)], None);
                g.string(a + 2, TEXT_KEY);
                g.string(a + 3, json);
            }
            // The raster layers share one empty array, after the first raster layer's block.
            None if !shared => {
                g.array(metadata_items, 0, KV);
                shared = true;
            }
            None => {}
        }
    }
    g.finish()
}

/// Write step 6: one deferred MemoryBlock of straight RGBA8 `rgba`, as BGRA in 256 KiB chunks,
/// each an independent gzip member (only one chunk is converted at a time).
fn write_pixels(out: &mut Vec<u8>, rgba: &[u8]) {
    const CHUNK: usize = 262144;
    out.push(0); // gzip
    out.extend((CHUNK as u32).to_be_bytes());
    let mut bgra = Vec::with_capacity(CHUNK.min(rgba.len()));
    // ponytail: chunks compress on one thread; Paint.NET compresses them in parallel (rayon on
    // native if large saves are slow; the bytes do not change).
    for (i, chunk) in (0u32..).zip(rgba.chunks(CHUNK)) {
        bgra.clear();
        bgra.extend(
            chunk
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0], p[3]]),
        );
        let member = gzip_member(&bgra);
        out.extend(i.to_be_bytes());
        out.extend((member.len() as u32).to_be_bytes());
        out.extend(member);
    }
}

/// Write step 3: the straight RGBA8 composite `rgba` (`w`×`h`) resized to `thumb_size` by
/// Bicubic in linear light, as a PNG with Auto depth. `None` when it cannot be made (the resize
/// refuses a buffer it cannot address, as on wasm for huge images): the thumbnail is optional.
fn thumbnail_png(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    use crate::resample::{Gamma, ResizeMode, resize};
    let (tw, th) = thumb_size(w, h);
    let data = resize(rgba, w, h, tw, th, ResizeMode::Bicubic, Gamma::Linear);
    let img = ImageData {
        width: tw,
        height: th,
        data,
    };
    super::encode::encode(&img, "png", &super::encode::SaveOptions::default_for("png")).ok()
}

/// Write steps 1–3: the `<pdnImage>` header XML, with `<thumb png=…>` in `<custom>` when there is
/// a thumbnail (without one it is exactly paintdotjs's header).
fn header_xml(w: u32, h: u32, n: usize, thumb_png: Option<&[u8]>) -> String {
    let thumb = thumb_png.map_or(String::new(), |p| {
        format!(r#"<thumb png="{}" />"#, base64_encode(p))
    });
    format!(
        r#"<pdnImage width="{w}" height="{h}" layers="{n}" savedWithVersion="5.112.9563.32325"><custom>{thumb}</custom></pdnImage>"#
    )
}

/// A text layer's `paint.rs:text` value (S§4.5): its spec and the hash of the pixels saved with
/// it. `None` for a raster layer.
fn text_metadata(layer: &Layer) -> Option<String> {
    let spec = layer.text.clone()?;
    let hash = format!("{:016x}", fnv1a(layer.surface.data()));
    serde_json::to_string(&TextMeta { v: 1, spec, hash }).ok()
}

/// Encodes `doc` as a Paint.NET 5.1.12 `.pdn` (§2.11 "Write"): the header with a thumbnail of
/// the composite, the NRBF graph with each layer's blendOp class, and the pixel blocks. Any
/// number of layers (no template cap).
pub fn encode(doc: &Document) -> Result<Vec<u8>> {
    let (w, h, n) = (doc.width, doc.height, doc.layers.size());
    if n == 0 {
        return err("document has no layers");
    }
    let png = thumbnail_png(doc.composite().data(), w, h);
    let xml = header_xml(w, h, n, png.as_deref());
    // A ≤ 256×256 thumbnail keeps the XML far below the 24-bit length limit.
    let mut out = b"PDN3".to_vec();
    out.extend(&(xml.len() as u32).to_le_bytes()[..3]);
    out.extend(xml.as_bytes());
    out.extend([0, 1]);
    let props: Vec<&LayerProperties> = doc.layers.iter().map(|l| &l.properties).collect();
    let texts: Vec<Option<String>> = doc.layers.iter().map(text_metadata).collect();
    out.extend(graph(w, h, doc.resolution, &props, &texts));
    for layer in doc.layers.iter() {
        write_pixels(&mut out, layer.surface.data());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn fixture() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/engine-ref/pdn-encode-3-layers.bin"
        ))
        .unwrap()
    }

    fn obj(class: &str, m: Vec<(&str, NrbfValue)>) -> NrbfValue {
        NrbfValue::Object {
            class: class.into(),
            members: m
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    fn int(n: i64) -> NrbfValue {
        NrbfValue::Primitive(nrbf::Prim::Int(n))
    }

    fn bytes_val(b: &[u8]) -> NrbfValue {
        NrbfValue::Bytes(b.into())
    }

    #[test]
    fn graph_end_is_where_pixel_blocks_start() {
        let js = fixture();
        let end = graph_end(&js).unwrap();
        assert_eq!(js[end], 0); // gzip block format byte
        assert_eq!(&js[end + 1..end + 5], 262144u32.to_be_bytes());
    }

    #[test]
    fn headerless_legacy_stream_decodes() {
        let js = fixture();
        let xml_len = js[4] as usize | (js[5] as usize) << 8 | (js[6] as usize) << 16;
        let a = decode(&js).unwrap();
        let b = decode(&js[7 + xml_len..]).unwrap();
        assert_eq!(b.header_xml, "");
        assert_eq!(a.layers.len(), b.layers.len());
        assert_eq!(a.layers[1].1.data, b.layers[1].1.data);
    }

    #[test]
    fn blend_op_class_and_legacy_opacity_win() {
        let layer = obj(
            "PaintDotNet.BitmapLayer",
            vec![
                (
                    "Layer+properties",
                    obj(
                        "p",
                        vec![
                            ("opacity", int(10)),
                            ("blendMode", obj("m", vec![("value__", int(7))])),
                        ],
                    ),
                ),
                (
                    "properties",
                    obj(
                        "q",
                        vec![
                            ("opacity", int(77)),
                            (
                                "blendOp",
                                obj("PaintDotNet.UserBlendOps+ScreenBlendOp", vec![]),
                            ),
                        ],
                    ),
                ),
            ],
        );
        let p = properties(&layer, 0);
        assert_eq!((p.opacity, p.blend_mode), (77, BlendMode::Screen));
        // -1 means "not set"; a null blendOp leaves blendMode alone; opacity defaults to 255.
        let layer = obj(
            "l",
            vec![
                (
                    "Layer+properties",
                    obj(
                        "p",
                        vec![("blendMode", obj("m", vec![("value__", int(7))]))],
                    ),
                ),
                ("properties", obj("q", vec![("opacity", int(-1))])),
            ],
        );
        let p = properties(&layer, 2);
        assert_eq!(
            (p.opacity, p.blend_mode, p.name.as_str()),
            (255, BlendMode::Overlay, "Layer 3")
        );
        // Unknown blendMode value -> Normal.
        let layer = obj(
            "l",
            vec![("Layer+properties", obj("p", vec![("blendMode", int(99))]))],
        );
        assert_eq!(properties(&layer, 0).blend_mode, BlendMode::Normal);
    }

    #[test]
    fn inline_memory_block_forms() {
        assert_eq!(
            &*inline_block(&obj("m", vec![("pointerData", bytes_val(&[1, 2, 3]))]), 3).unwrap(),
            [1, 2, 3]
        );
        let chunked = obj(
            "m",
            vec![
                ("chunkCount", int(2)),
                ("chunkFormat", int(0)),
                ("chunk0", bytes_val(&[1, 2])),
                ("chunk1", bytes_val(&[3])),
            ],
        );
        assert_eq!(&*inline_block(&chunked, 3).unwrap(), [1, 2, 3]);
        assert!(inline_block(&chunked, 4).is_err()); // lengths must sum to length
        assert!(inline_block(&chunked, 2).is_err());
        let bad_fmt = obj("m", vec![("chunkCount", int(0)), ("chunkFormat", int(1))]);
        assert!(inline_block(&bad_fmt, 0).is_err());
    }

    #[test]
    fn gzip_member_round_trips_and_has_the_5_1_12_header() {
        let data: Vec<u8> = (0..5000u32).map(|i| (i * 7) as u8).collect();
        let m = gzip_member(&data);
        assert_eq!(m[..10], GZIP_MEMBER_HEADER);
        assert_eq!(gunzip_limited(&m, data.len()).unwrap(), data);
        assert!(gunzip_limited(&m, data.len() - 1).is_err());
        // CRC32 check value of "123456789".
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn zip_bombs_and_oversized_blocks_are_refused() {
        // A chunk that inflates past its slot.
        let bomb = gzip_member(&vec![0u8; 1 << 20]);
        let mut b = vec![0u8];
        b.extend(16u32.to_be_bytes());
        b.extend(0u32.to_be_bytes());
        b.extend((bomb.len() as u32).to_be_bytes());
        b.extend(&bomb);
        assert!(read_deferred_block(&b, 16).is_err());
        // Chunk size 0, bad format byte, absurd length.
        assert!(read_deferred_block(&[0, 0, 0, 0, 0], 8).is_err());
        assert!(read_deferred_block(&[7, 0, 0, 0, 4], 8).is_err());
        assert!(read_deferred_block(&[0, 0, 0, 0, 4], usize::MAX / 2).is_err());
        assert!(read_deferred_block(&[], 8).is_err());
        // A chunk length running off the end.
        assert!(read_deferred_block(&[1, 0, 0, 0, 8, 0, 0, 0, 0, 255, 255, 255, 255], 8).is_err());
    }

    #[test]
    fn oversized_dimensions_are_refused() {
        let js = fixture();
        // Patch every surface/document width of 19 (LE i32) to 40000.
        let mut b = js.clone();
        let end = graph_end(&b).unwrap();
        for i in 0..end.saturating_sub(4) {
            if b[i..i + 4] == 19i32.to_le_bytes() {
                b[i..i + 4].copy_from_slice(&40000i32.to_le_bytes());
            }
        }
        assert!(decode(&b).is_err());
    }

    use super::base64_encode as b64;

    #[test]
    fn paintdotjs_json_files_decode() {
        let mut pm = tiny_skia::Pixmap::new(2, 1).unwrap();
        pm.pixels_mut()[0] = tiny_skia::PremultipliedColorU8::from_rgba(100, 0, 0, 100).unwrap();
        pm.pixels_mut()[1] = tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap();
        let png = b64(&pm.encode_png().unwrap());
        let json = format!(
            r#"{{"format":"paint.js.pdn/1","width":3,"height":1,"resolution":120,"activeLayer":0,
            "layers":[{{"name":"A","visible":false,"isBackground":false,"opacity":128,"blendMode":"screen","png":"data:image/png;base64,{png}"}},
                      {{"name":"","blendMode":"bogus","png":"data:image/png;base64,{png}"}}]}}"#
        );
        let d = decode(json.as_bytes()).unwrap();
        assert_eq!(
            (d.width, d.height, d.resolution, d.layers.len()),
            (3, 1, 120.0, 2)
        );
        let (p, img) = &d.layers[0];
        assert_eq!(
            (p.name.as_str(), p.visible, p.opacity, p.blend_mode),
            ("A", false, 128, BlendMode::Screen)
        );
        assert_eq!(&img.data[..4], [255, 0, 0, 100]);
        assert_eq!(&img.data[8..], [0, 0, 0, 0]); // outside the PNG
        assert_eq!(
            (d.layers[1].0.name.as_str(), d.layers[1].0.blend_mode),
            ("Layer 2", BlendMode::Normal)
        );
        for bad in [
            r#"{"format":"x"}"#,
            r#"{"format":"paint.js.pdn/1","width":0,"height":1,"layers":[]}"#,
            r#"{"format":"paint.js.pdn/1","width":99999,"height":1,"layers":[]}"#,
            r#"{"format":"paint.js.pdn/1","width":1,"height":1,"layers":[]}"#,
            r#"{"format":"paint.js.pdn/1","width":1,"height":1,"layers":[{"png":"data:image/png;base64,AAAA"}]}"#,
            "not json",
            "",
        ] {
            assert!(decode(bad.as_bytes()).is_err(), "{bad}");
        }
    }

    /// Template `n` from `assets/pdn/templates/`: test fixtures that stay on the developer's disk
    /// and are not published (Ruling R222). Without the set (template 1 absent) the test is
    /// skipped with a note; once it is present every template is required, and any other read
    /// error fails.
    fn template(n: usize) -> Option<Vec<u8>> {
        let path = |n: usize| {
            format!(
                "{}/../../assets/pdn/templates/{n}.bin",
                env!("CARGO_MANIFEST_DIR")
            )
        };
        match std::fs::read(path(n)) {
            Ok(bytes) => Some(bytes),
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    && matches!(std::path::Path::new(&path(1)).try_exists(), Ok(false)) =>
            {
                eprintln!("skipped: {} is missing (local-only test fixture)", path(1));
                None
            }
            Err(e) => panic!("{}: {e}", path(n)),
        }
    }

    /// The 64 templates are real Paint.NET 5.1.12 graphs (N layers each): the reader consumes
    /// every byte and finds N BitmapLayers with a NormalBlendOp.
    #[test]
    fn paint_net_templates_parse_completely() {
        for n in 1..=64usize {
            let Some(bytes) = template(n) else {
                return;
            };
            let (root, used) = nrbf::deserialize(&bytes).unwrap();
            assert_eq!(used, bytes.len(), "template {n}");
            assert_eq!(
                root.get("layers").get("ArrayList+_size").as_i64(),
                Some(n as i64)
            );
            let layer = &root
                .get("layers")
                .get("ArrayList+_items")
                .as_array()
                .unwrap()[n - 1];
            assert_eq!(
                layer.get("properties").get("blendOp").class(),
                Some("PaintDotNet.UserBlendOps+NormalBlendOp")
            );
        }
    }

    #[test]
    fn resolution_comes_from_exif_metadata() {
        let item = |id: &str, bytes: &[u8]| {
            obj(
                "kv",
                vec![(
                    "value",
                    NrbfValue::String(
                        format!(
                            r#"<exif id="{id}" len="8" type="5" value="{}" />"#,
                            b64(bytes)
                        )
                        .into(),
                    ),
                )],
            )
        };
        let mut x = 300u32.to_le_bytes().to_vec();
        x.extend(1u32.to_le_bytes());
        let root = obj(
            "d",
            vec![(
                "userMetadataItems",
                NrbfValue::Array(vec![item("296", &[2, 0]), item("282", &x)]),
            )],
        );
        assert_eq!(resolution_of(&root), 300.0);
        assert_eq!(resolution_of(&obj("d", vec![])), 96.0);
    }

    // ---- fix round 1 ----

    fn put_str(b: &mut Vec<u8>, s: &str) {
        let mut n = s.len();
        loop {
            let byte = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                b.push(byte);
                break;
            }
            b.push(byte | 0x80);
        }
        b.extend(s.as_bytes());
    }

    enum V {
        I32(i32),
        I64(i64),
        Bool(bool),
        Rec(Vec<u8>),
    }

    /// A ClassWithMembersAndTypes record whose members are Int32/Int64/Boolean or nested records.
    fn class_rec(id: i32, name: &str, members: Vec<(&str, V)>) -> Vec<u8> {
        let mut b = vec![5];
        b.extend(id.to_le_bytes());
        put_str(&mut b, name);
        b.extend((members.len() as i32).to_le_bytes());
        for (n, _) in &members {
            put_str(&mut b, n);
        }
        for (_, v) in &members {
            b.push(match v {
                V::Rec(_) => 2,
                _ => 0,
            });
        }
        for (_, v) in &members {
            match v {
                V::I32(_) => b.push(8),
                V::I64(_) => b.push(9),
                V::Bool(_) => b.push(1),
                V::Rec(_) => {}
            }
        }
        b.extend(0i32.to_le_bytes()); // library id
        for (_, v) in members {
            match v {
                V::I32(n) => b.extend(n.to_le_bytes()),
                V::I64(n) => b.extend(n.to_le_bytes()),
                V::Bool(x) => b.push(x as u8),
                V::Rec(r) => b.extend(r),
            }
        }
        b
    }

    fn string_rec(id: i32, s: &str) -> Vec<u8> {
        let mut b = vec![6];
        b.extend(id.to_le_bytes());
        put_str(&mut b, s);
        b
    }

    fn byte_array_rec(id: i32, data: &[u8]) -> Vec<u8> {
        let mut b = vec![15];
        b.extend(id.to_le_bytes());
        b.extend((data.len() as i32).to_le_bytes());
        b.push(2);
        b.extend(data);
        b
    }

    /// A one-layer headerless legacy file (`00 01` + NRBF) whose MemoryBlock is inline `pointerData`.
    fn inline_file(w: i32, h: i32, bgra: &[u8]) -> Vec<u8> {
        inline_file_with(w, h, bgra, "inline", 1, vec![])
    }

    /// Like `inline_file`, with a chosen layer name, `refs` list entries (the first is the layer,
    /// the rest MemberReferences to it) and extra Document members.
    fn inline_file_with(
        w: i32,
        h: i32,
        bgra: &[u8],
        name: &str,
        refs: usize,
        extra: Vec<(&str, V)>,
    ) -> Vec<u8> {
        let mb = class_rec(
            6,
            "PaintDotNet.MemoryBlock",
            vec![
                ("length64", V::I64(bgra.len() as i64)),
                ("deferred", V::Bool(false)),
                ("pointerData", V::Rec(byte_array_rec(7, bgra))),
            ],
        );
        let surface = class_rec(
            5,
            "PaintDotNet.Surface",
            vec![
                ("width", V::I32(w)),
                ("height", V::I32(h)),
                ("stride", V::I32(w * 4)),
                ("scan0", V::Rec(mb)),
            ],
        );
        let props = class_rec(
            8,
            "Layer+LayerProperties",
            vec![("name", V::Rec(string_rec(9, name)))],
        );
        let layer = class_rec(
            4,
            "PaintDotNet.BitmapLayer",
            vec![
                ("surface", V::Rec(surface)),
                ("Layer+properties", V::Rec(props)),
            ],
        );
        let mut items = vec![16];
        items.extend(3i32.to_le_bytes());
        items.extend((refs as i32).to_le_bytes());
        items.extend(layer);
        for _ in 1..refs {
            items.push(9);
            items.extend(4i32.to_le_bytes());
        }
        let list = class_rec(
            2,
            "PaintDotNet.LayerList",
            vec![
                ("ArrayList+_items", V::Rec(items)),
                ("ArrayList+_size", V::I32(refs as i32)),
            ],
        );
        let mut doc_members = vec![
            ("width", V::I32(w)),
            ("height", V::I32(h)),
            ("layers", V::Rec(list)),
        ];
        doc_members.extend(extra);
        let doc = class_rec(1, "PaintDotNet.Document", doc_members);
        let mut f = vec![0, 1, 0];
        for v in [1i32, 0, 1, 0] {
            f.extend(v.to_le_bytes());
        }
        f.extend(doc);
        f.push(11);
        f
    }

    #[test]
    fn inline_1080p_layer_decodes() {
        let (w, h) = (1920usize, 1080usize);
        let bgra: Vec<u8> = (0..w * h * 4).map(|i| (i * 31 % 251) as u8).collect();
        let d = decode(&inline_file(w as i32, h as i32, &bgra)).unwrap();
        assert_eq!(
            (d.width, d.height, d.layers[0].0.name.as_str()),
            (1920, 1080, "inline")
        );
        let rgba = &d.layers[0].1.data;
        assert_eq!(rgba.len(), bgra.len());
        assert_eq!(rgba[..4], [bgra[2], bgra[1], bgra[0], bgra[3]]);
        let last = bgra.len() - 4;
        assert_eq!(
            rgba[last..],
            [bgra[last + 2], bgra[last + 1], bgra[last], bgra[last + 3]]
        );
    }

    /// Review probe `layername`: one layer with a 1 MiB name, referenced 4000 times. Consumers
    /// copy the name per reference, so the reader must refuse before that happens.
    #[test]
    fn shared_layer_name_references_are_refused_quickly() {
        let big = "n".repeat(1 << 20);
        assert!(decode(&inline_file_with(1, 1, &[0; 4], &big, 4000, vec![])).is_err());
        // A couple of references to a long name are fine (the layers share their pixels' graph).
        assert!(decode(&inline_file_with(1, 1, &[0; 4], &big, 2, vec![])).is_ok());
    }

    /// Review probe `meta`: 90000 references to one metadata item with a 16 KiB value, each of
    /// which `resolution_of` would scan.
    #[test]
    fn shared_exif_items_are_refused_quickly() {
        let kv = class_rec(
            20,
            "System.Collections.Generic.KeyValuePair",
            vec![
                ("key", V::Rec(string_rec(21, "$exif.tag0[0]"))),
                ("value", V::Rec(string_rec(22, &"v".repeat(16 << 10)))),
            ],
        );
        let mut items = vec![16];
        items.extend(30i32.to_le_bytes());
        items.extend(90_000i32.to_le_bytes());
        items.extend(kv);
        for _ in 1..90_000 {
            items.push(9);
            items.extend(20i32.to_le_bytes());
        }
        let t = std::time::Instant::now();
        assert!(
            decode(&inline_file_with(
                1,
                1,
                &[0; 4],
                "a",
                1,
                vec![("userMetadataItems", V::Rec(items))]
            ))
            .is_err()
        );
        assert!(t.elapsed().as_secs() < 10);
    }

    #[test]
    fn graph_end_applies_the_early_paintdotjs_repair() {
        let js = fixture();
        let end = graph_end(&js).unwrap();
        let xml_len = js[4] as usize | (js[5] as usize) << 8 | (js[6] as usize) << 16;
        let mut broken = js.clone();
        for (i, mode) in [0i32, 12, 0].into_iter().enumerate() {
            let g = &mut broken[7 + xml_len + 2..end];
            let rec = legacy_record_offset(g, 3, i).unwrap();
            g[rec..rec + 4].copy_from_slice(&mode.to_le_bytes());
        }
        assert_eq!(graph_end(&broken), Some(end));
    }

    #[test]
    fn alpha_zero_colour_survives_decode() {
        let d = decode(&inline_file(1, 1, &[30, 20, 10, 0])).unwrap();
        assert_eq!(d.layers[0].1.data, [10, 20, 30, 0]);
    }

    #[test]
    fn unknown_blend_op_class_is_normal() {
        let layer = obj(
            "l",
            vec![
                (
                    "Layer+properties",
                    obj(
                        "p",
                        vec![("blendMode", obj("m", vec![("value__", int(12))]))],
                    ),
                ),
                (
                    "properties",
                    obj("q", vec![("blendOp", obj("Some.Other.Op", vec![]))]),
                ),
            ],
        );
        assert_eq!(properties(&layer, 0).blend_mode, BlendMode::Normal);
    }

    #[test]
    fn gzip_trailer_is_verified() {
        let m = gzip_member(&[1, 2, 3, 4]);
        assert!(gunzip_limited(&m, 4).is_ok());
        let mut bad_crc = m.clone();
        let n = bad_crc.len();
        bad_crc[n - 8] ^= 1;
        assert!(gunzip_limited(&bad_crc, 4).is_err());
        let mut bad_size = m.clone();
        bad_size[n - 1] ^= 1;
        assert!(gunzip_limited(&bad_size, 4).is_err());
        assert!(gunzip_limited(&m[..n - 8], 4).is_err());
        assert!(gunzip_limited(&m[..n - 4], 4).is_err());
    }

    #[test]
    fn chunk_count_beyond_the_input_is_refused_before_allocating() {
        // 1 GiB claimed with 1-byte chunks: 2^30 chunks cannot fit in 13 bytes.
        let b = [0u8, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1];
        assert!(read_deferred_block(&b, 1 << 30).is_err());
    }

    #[test]
    fn png_header_dimension_overflow_is_refused() {
        let mut png = vec![
            0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
        ];
        png.extend([0xff; 8]);
        png.extend([0; 8]);
        let json = format!(
            r#"{{"format":"paint.js.pdn/1","width":1,"height":1,"layers":[{{"png":"data:image/png;base64,{}"}}]}}"#,
            b64(&png)
        );
        assert!(decode(json.as_bytes()).is_err());
    }

    /// Ruling R41: real Paint.NET 5.1.12 writes its version into `Document.savedWith` (BB:
    /// icon-lemon.pdn holds 5.112.9563.32325); the templates hold 0.0.0.0. Writes the four
    /// `System.Version` Int32s into a template: they follow the record's metadata, which ends
    /// with "_Revision" and four Int32 type bytes.
    fn with_saved_with(mut t: Vec<u8>) -> Vec<u8> {
        let meta = b"\x09_Revision\x00\x00\x00\x00\x08\x08\x08\x08";
        let at = t.windows(meta.len()).position(|w| w == meta).unwrap() + meta.len();
        assert_eq!(
            t[at..at + 16],
            [0; 16],
            "the template's savedWith is 0.0.0.0"
        );
        for (k, v) in [5i32, 112, 9563, 32325].into_iter().enumerate() {
            t[at + 4 * k..at + 4 * k + 4].copy_from_slice(&v.to_le_bytes());
        }
        t
    }

    /// The writer reproduces every real 5.1.12 template byte for byte, savedWith aside
    /// (`with_saved_with`): an all-Normal N-layer 257×263 document whose layers are named
    /// `PDJ_LAYER_<i>` (the templates' placeholders).
    #[test]
    fn graph_matches_every_template() {
        for n in 1..=64usize {
            let Some(t) = template(n).map(with_saved_with) else {
                return;
            };
            let props: Vec<LayerProperties> = (0..n)
                .map(|i| LayerProperties::new(&format!("PDJ_LAYER_{i:04}"), true, false, 255))
                .collect();
            let props: Vec<&LayerProperties> = props.iter().collect();
            assert!(graph(257, 263, 96.0, &props, &[]) == t, "template {n}");
        }
    }

    /// Write step 3: the embedded thumbnail is the composite resized to `thumb_size` by Bicubic
    /// in linear light, PNG-encoded without loss. A near-square portrait over 256 px checks the
    /// orientation (300×301 → 255×256).
    #[test]
    fn thumbnail_is_the_resized_composite() {
        use crate::resample::{Gamma, ResizeMode, resize};
        let (w, h) = (300, 301);
        let mut doc = Document::new(w, h, 96.0);
        for (seed, mode) in [(3usize, BlendMode::Normal), (7, BlendMode::Multiply)] {
            let mut l = Layer::create_layer(w, h, "l");
            for (i, b) in l.surface.data_mut().iter_mut().enumerate() {
                *b = ((i * seed + i / 1201 * 13) % 256) as u8;
            }
            l.properties.blend_mode = mode;
            l.properties.opacity = 200;
            doc.add_layer(l);
        }
        let xml = decode(&encode(&doc).unwrap()).unwrap().header_xml;
        let png = base64_decode(xml_attr(&xml, "png").unwrap()).unwrap();
        let thumb = crate::io::codecs::decode("png", &png).unwrap().image;
        let (tw, th) = thumb_size(w, h);
        assert_eq!((thumb.width, thumb.height, tw, th), (255, 256, 255, 256));
        let composite = doc.composite();
        let want = resize(
            composite.data(),
            w,
            h,
            tw,
            th,
            ResizeMode::Bicubic,
            Gamma::Linear,
        );
        assert!(thumb.data == want);
    }

    /// §5: the thumbnail is optional. When the resize refuses (wasm's `resample::accepts` on a
    /// huge image; here an empty source) there is no PNG, and the header keeps an empty
    /// `<custom>`: exactly paintdotjs's header.
    #[test]
    fn save_without_a_thumbnail_keeps_an_empty_custom() {
        assert_eq!(thumbnail_png(&[], 300, 300), None);
        let js = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/engine-ref/pdn-encode-3-normal.bin"
        ))
        .unwrap();
        let xml_len = js[4] as usize | (js[5] as usize) << 8 | (js[6] as usize) << 16;
        assert_eq!(header_xml(19, 11, 3, None).as_bytes(), &js[7..7 + xml_len]);
    }

    /// The value budget follows the file's size; a whole-gzip graph is budgeted by its
    /// compressed bytes, so a small file inflating into 400k references is refused by the
    /// reader (an inflated-size budget would accept it).
    #[test]
    fn gzip_graph_budget_follows_the_compressed_size() {
        let mut g = vec![0u8];
        for v in [1i32, -1, 1, 0] {
            g.extend(v.to_le_bytes());
        }
        g.push(4); // id 2: class X, no members
        g.extend(2i32.to_le_bytes());
        put_str(&mut g, "X");
        g.extend(0i32.to_le_bytes());
        g.push(16); // root: object[400000], every element a reference to X
        g.extend(1i32.to_le_bytes());
        g.extend(400_000i32.to_le_bytes());
        for _ in 0..400_000 {
            g.push(9);
            g.extend(2i32.to_le_bytes());
        }
        g.push(11);
        assert!(nrbf::deserialize(&g).is_ok()); // 2 MB of records earn a 500k-value budget
        let file = gzip_member(&g);
        assert!(file.len() < 1 << 20);
        assert_eq!(
            decode(&file).unwrap_err().0,
            NrbfError::TooLarge.to_string()
        );
    }

    /// The closed-form per-layer ids used by the repair equal the real 5.1.12 templates.
    #[test]
    fn repair_bytes_match_all_templates() {
        for n in 1..=64usize {
            let Some(t) = template(n) else {
                return;
            };
            for i in 0..n {
                let m = format!("PDJ_LAYER_{i:04}");
                let at = t.windows(m.len()).position(|w| w == m.as_bytes()).unwrap();
                let (sig, rec) = template_layer_bytes(n, i);
                assert_eq!(t[at - 6..at - 1], sig, "n {n} layer {i}");
                let r = at + m.len() + 5 + 3;
                assert_eq!(t[r..r + 4], rec, "n {n} layer {i}");
            }
        }
    }

    /// A file shaped like an early paintdotjs one: each layer's blend mode written over the four
    /// bytes of its enum record header. The strict parse fails; the repair restores the graph.
    #[test]
    fn early_paintdotjs_blend_records_are_repaired() {
        let js = fixture();
        let start = graph_end(&js).unwrap();
        let xml_len = js[4] as usize | (js[5] as usize) << 8 | (js[6] as usize) << 16;
        let graph = &js[7 + xml_len + 2..start];
        let n = 3;
        // The fixture holds Normal / Screen(12) / Normal; damage it the early-build way.
        let mut broken = graph.to_vec();
        for (i, mode) in [0i32, 12, 0].into_iter().enumerate() {
            let rec = legacy_record_offset(&broken, n, i).unwrap();
            let value_at = blend_value_offset(&broken, rec).unwrap();
            assert_eq!(
                i32::from_le_bytes(broken[value_at..value_at + 4].try_into().unwrap()),
                mode
            );
            broken[rec..rec + 4].copy_from_slice(&mode.to_le_bytes());
        }
        assert!(nrbf::deserialize(&broken).is_err());
        assert_eq!(repair_legacy_blend_records(&broken, n).unwrap(), graph);
        // End to end: the whole file decodes.
        let mut file = js[..7 + xml_len + 2].to_vec();
        file.extend(&broken);
        file.extend(&js[start..]);
        let d = decode(&file).unwrap();
        assert_eq!(d.layers.len(), 3);
        assert_eq!(d.layers[1].0.name, "Ünïcode layer");
        // Damage that is not a blend mode is not repaired.
        let mut junk = broken.clone();
        let rec = legacy_record_offset(&junk, n, 0).unwrap();
        junk[rec..rec + 4].copy_from_slice(&99i32.to_le_bytes());
        assert!(repair_legacy_blend_records(&junk, n).is_none());
        assert!(repair_legacy_blend_records(&broken[..40], n).is_none());
    }

    // ---- S§4.5 text layers (PC17) -----------------------------------------------------------

    fn hi() -> TextSpec {
        TextSpec {
            text: "Hi".into(),
            origin: (4.0, 6.0),
            color: [10, 20, 30, 255],
            ..Default::default()
        }
    }

    /// A white background and, above it, a text layer holding `spec` over `surface`.
    fn text_doc(spec: &TextSpec, surface: crate::document::surface::Surface) -> Document {
        let mut doc = Document::new(40, 30, 96.0);
        doc.add_layer(Layer::create_background_layer(40, 30));
        let mut l = Layer::create_layer(40, 30, "Text");
        l.surface = surface;
        l.text = Some(spec.clone());
        doc.add_layer(l);
        doc
    }

    fn rendered() -> Document {
        let surface = crate::document::text_layer::render(&hi(), 40, 30);
        assert!(surface.data().chunks(4).any(|p| p[3] > 0), "no ink");
        text_doc(&hi(), surface)
    }

    fn graph_of(bytes: &[u8]) -> NrbfValue {
        let xml_len = bytes[4] as usize | (bytes[5] as usize) << 8 | (bytes[6] as usize) << 16;
        let end = graph_end(bytes).unwrap();
        nrbf::deserialize(&bytes[7 + xml_len + 2..end]).unwrap().0
    }

    /// S§4.5: a document without a text layer keeps today's bytes: through the whole `encode`
    /// path every layer still shares one empty metadata array, so the graph is the template's.
    #[test]
    fn pdn_without_text_is_byte_identical() {
        for n in [1usize, 2, 64] {
            let Some(t) = template(n).map(with_saved_with) else {
                return;
            };
            let mut doc = Document::new(257, 263, 96.0);
            for i in 0..n {
                doc.add_layer(Layer::create_layer(257, 263, &format!("PDJ_LAYER_{i:04}")));
            }
            let bytes = encode(&doc).unwrap();
            let xml_len = bytes[4] as usize | (bytes[5] as usize) << 8 | (bytes[6] as usize) << 16;
            assert!(
                bytes[7 + xml_len + 2..graph_end(&bytes).unwrap()] == t,
                "template {n}"
            );
        }
    }

    /// S§4.5: the text layer is an ordinary bitmap layer whose own metadata holds one
    /// `paint.rs:text` entry (the background keeps the shared empty array); it loads editable
    /// with the same pixels.
    #[test]
    fn text_layer_round_trips() {
        let doc = rendered();
        let bytes = encode(&doc).unwrap();
        let items = graph_of(&bytes)
            .get("layers")
            .get("ArrayList+_items")
            .clone();
        let meta = |i: usize| {
            items.as_array().unwrap()[i]
                .get("Layer+properties")
                .get("userMetadataItems")
                .as_array()
                .unwrap()
                .to_vec()
        };
        assert!(meta(0).is_empty());
        assert_eq!(meta(1).len(), 1);
        assert_eq!(meta(1)[0].get("key").as_str(), Some(TEXT_KEY));
        let back = decode(&bytes).unwrap().to_document();
        for i in 0..2 {
            let (a, b) = (
                doc.layers.get_at(i).unwrap(),
                back.layers.get_at(i).unwrap(),
            );
            assert_eq!(a.properties, b.properties, "layer {i}");
            assert_eq!(a.text, b.text, "layer {i}");
            assert!(a.surface.data() == b.surface.data(), "layer {i}");
        }
        assert_eq!(back.layers.get_at(1).unwrap().text, Some(hi()));
    }

    /// S§4.5: Paint.NET keeps the layer's metadata when it edits the pixels; one changed byte
    /// makes the hash stale, so the layer loads as the bitmap it now is.
    #[test]
    fn stale_text_spec_loads_as_bitmap() {
        let doc = rendered();
        let bytes = encode(&doc).unwrap();
        assert_eq!(decode(&bytes).unwrap().texts[1], Some(hi()));
        let mut edited = bytes[..graph_end(&bytes).unwrap()].to_vec();
        write_pixels(&mut edited, doc.layers.get_at(0).unwrap().surface.data());
        let mut px = doc.layers.get_at(1).unwrap().surface.data().to_vec();
        px[(10 * 40 + 10) * 4] ^= 1;
        write_pixels(&mut edited, &px);
        let back = decode(&edited).unwrap();
        assert!(back.layers[1].1.data == px);
        assert_eq!(back.texts[1], None);
        assert_eq!(back.to_document().layers.get_at(1).unwrap().text, None);
    }

    /// S§4.5 "Fonts": loading never re-renders. A spec naming a missing font over pixels no
    /// render would produce loads editable with exactly the saved pixels.
    #[test]
    fn text_layer_not_rerendered_on_load() {
        let mut spec = hi();
        spec.settings.family = "No Such Font PC17".into();
        let mut surface = crate::document::surface::Surface::create(40, 30);
        for (i, b) in surface.data_mut().iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        let doc = text_doc(&spec, surface);
        let back = decode(&encode(&doc).unwrap()).unwrap().to_document();
        let (a, b) = (
            doc.layers.get_at(1).unwrap(),
            back.layers.get_at(1).unwrap(),
        );
        assert_eq!(b.text.as_ref(), Some(&spec));
        assert!(a.surface.data() == b.surface.data());
    }

    /// Other applications' (or later versions') metadata keys are skipped; only `paint.rs:text`
    /// is read.
    #[test]
    fn unknown_metadata_keys_are_ignored() {
        let doc = rendered();
        let layer = doc.layers.get_at(1).unwrap();
        let json = text_metadata(layer).unwrap();
        let kv = |k: &str, v: &str| {
            obj(
                KV,
                vec![
                    ("key", NrbfValue::String(k.into())),
                    ("value", NrbfValue::String(v.into())),
                ],
            )
        };
        let with = |items| {
            obj(
                "PaintDotNet.BitmapLayer",
                vec![(
                    "Layer+properties",
                    obj(
                        "PaintDotNet.Layer+LayerProperties",
                        vec![("userMetadataItems", NrbfValue::Array(items))],
                    ),
                )],
            )
        };
        let px = layer.surface.data();
        let mixed = vec![
            kv("other.app:thing", "{}"),
            kv(TEXT_KEY, &json),
            kv("zz", "x"),
        ];
        assert_eq!(text_spec(&with(mixed), px), Some(hi()));
        assert_eq!(
            text_spec(&with(vec![kv("other.app:text", &json)]), px),
            None
        );
        assert_eq!(text_spec(&with(vec![]), px), None);
    }

    /// A damaged `paint.rs:text` value costs only the editability: the file opens and the layer
    /// is a bitmap with its pixels.
    #[test]
    fn malformed_text_metadata_loads_as_bitmap() {
        let doc = rendered();
        let mut bytes = encode(&doc).unwrap();
        let at = bytes
            .windows(6)
            .position(|w| w == br#"{"v":1"#)
            .expect("the spec JSON is in the graph");
        bytes[at] = b'x';
        let back = decode(&bytes).unwrap().to_document();
        let (a, b) = (
            doc.layers.get_at(1).unwrap(),
            back.layers.get_at(1).unwrap(),
        );
        assert_eq!(b.text, None);
        assert!(a.surface.data() == b.surface.data());
    }

    /// [text, raster, text]: the shared empty array moves to the first raster layer (1), and the
    /// two text layers' arrays step to distinct ids. The file decodes with both specs back, each
    /// on its own layer, and every layer's pixels.
    #[test]
    fn text_raster_text_round_trips() {
        let yo = TextSpec {
            text: "Yo".into(),
            ..hi()
        };
        let text = |spec: &TextSpec| {
            let mut l = Layer::create_layer(40, 30, "Text");
            l.surface = crate::document::text_layer::render(spec, 40, 30);
            l.text = Some(spec.clone());
            l
        };
        let mut doc = Document::new(40, 30, 96.0);
        doc.add_layer(text(&yo));
        doc.add_layer(Layer::create_background_layer(40, 30));
        doc.add_layer(text(&hi()));
        let bytes = encode(&doc).unwrap();
        let items = graph_of(&bytes)
            .get("layers")
            .get("ArrayList+_items")
            .clone();
        let lens: Vec<usize> = items.as_array().unwrap()[..3]
            .iter()
            .map(|l| {
                let meta = l.get("Layer+properties").get("userMetadataItems");
                meta.as_array().unwrap().len()
            })
            .collect();
        assert_eq!(lens, [1, 0, 1]);
        let back = decode(&bytes).unwrap();
        assert_eq!(back.texts, [Some(yo), None, Some(hi())]);
        let back = back.to_document();
        for i in 0..3 {
            let (a, b) = (
                doc.layers.get_at(i).unwrap(),
                back.layers.get_at(i).unwrap(),
            );
            assert_eq!(a.properties, b.properties, "layer {i}");
            assert_eq!(a.text, b.text, "layer {i}");
            assert!(a.surface.data() == b.surface.data(), "layer {i}");
        }
    }

    /// A later format version (`"v": 2`) is not misread: the layer loads as a bitmap.
    #[test]
    fn text_metadata_v2_loads_as_bitmap() {
        let doc = rendered();
        let mut bytes = encode(&doc).unwrap();
        let at = bytes
            .windows(6)
            .position(|w| w == br#"{"v":1"#)
            .expect("the spec JSON is in the graph");
        bytes[at + 5] = b'2';
        let back = decode(&bytes).unwrap();
        assert_eq!(back.texts[1], None);
        let back = back.to_document();
        let (a, b) = (
            doc.layers.get_at(1).unwrap(),
            back.layers.get_at(1).unwrap(),
        );
        assert_eq!(b.text, None);
        assert!(a.surface.data() == b.surface.data());
    }
}
