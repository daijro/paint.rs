//! AssetPackManager.js and docs/asset-packs.md, paintdotjs b4e6fc5.
//! Spec §0.6; R101 whole-archive validation and R103 pack-only image expansion limits.
use image::ImageDecoder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
};
pub const MAX_ZIP_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 1000;
pub const MAX_EXTRACTED: u64 = 60 * 1024 * 1024;
pub const MAX_IMAGE_RGBA: u64 = 64 * 1024 * 1024;
pub const MAX_PACK_RGBA: u64 = 256 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetPack {
    /// Manifest identity, distinct from the installed record's fresh UUID identity.
    pub id: String,
    pub manifest: Value,
    pub files: BTreeMap<String, Vec<u8>>,
    pub translations: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackError(pub String);
impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PackError {}
fn error(e: impl std::fmt::Display) -> PackError {
    PackError(e.to_string())
}
type Result<T> = std::result::Result<T, PackError>;
/// R101: canonicalize every archive entry, including ignored entries and directories.
pub fn canonical_path(name: &str) -> Result<String> {
    let name = name.replace('\\', "/");
    if name.starts_with('/')
        || name.contains('\0')
        || (name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && name.as_bytes().get(1) == Some(&b':'))
    {
        return Err(error("Invalid absolute, drive or NUL path"));
    }
    let mut parts = vec![];
    for part in name.split('/') {
        match part {
            ".." => return Err(error("Parent traversal is forbidden")),
            "" | "." => {}
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        return Err(error("Empty archive path"));
    }
    let first = parts[0].as_bytes();
    if first.first().is_some_and(u8::is_ascii_alphabetic) && first.get(1) == Some(&b':') {
        return Err(error("Drive path after canonical normalization"));
    }
    Ok(parts.join("/"))
}
pub fn validate_manifest(value: &Value) -> Result<()> {
    let m = value
        .as_object()
        .ok_or_else(|| error("asset-pack.json must contain an object"))?;
    if m.get("formatVersion").and_then(Value::as_f64) != Some(1.) {
        return Err(error("Only formatVersion 1 is supported"));
    }
    let name = m
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| error("Missing pack name"))?;
    if name.trim().is_empty() || name.encode_utf16().count() > 80 {
        return Err(error("Pack name must contain 1 to 80 UTF-16 units"));
    }
    for field in ["author", "version", "description", "id"] {
        if m.get(field).is_some_and(|v| !v.is_string()) {
            return Err(error(format!("{field} must be a string")));
        }
    }
    Ok(())
}
fn image_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    ["icons/", "images/", "cursors/"]
        .iter()
        .any(|p| lower.starts_with(p))
        && (lower.ends_with(".png") || lower.ends_with(".webp"))
}
// TextDecoder, used by the JS importer, consumes a leading UTF-8 BOM.
fn json_bytes(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes)
}
fn locale(path: &str) -> Option<&str> {
    let code = path.strip_prefix("lang/")?.strip_suffix(".json")?;
    (!code.is_empty() && code.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'))
        .then_some(code)
}
/// Sequential validation: header RGBA guard before decoder/output allocation, then verify output.
pub fn decode_image(bytes: &[u8]) -> Result<image::RgbaImage> {
    decode_with_budget(bytes, MAX_IMAGE_RGBA)
}
fn decode_with_budget(bytes: &[u8], remaining: u64) -> Result<image::RgbaImage> {
    let format = image::guess_format(bytes).map_err(error)?;
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::WebP) {
        return Err(error("Pack images must be PNG or WebP"));
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(crate::io::codecs::MAX_DECODE_SIDE);
    limits.max_image_height = Some(crate::io::codecs::MAX_DECODE_SIDE);
    limits.max_alloc = Some(MAX_IMAGE_RGBA);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(error)?;
    let (w, h) = decoder.dimensions();
    let rgba = crate::io::codecs::check_size(w, h).map_err(error)? as u64;
    if rgba > MAX_IMAGE_RGBA || decoder.total_bytes() > MAX_IMAGE_RGBA {
        return Err(error("Pack image exceeds 64 MiB decoded RGBA"));
    }
    if rgba > remaining {
        return Err(error("Pack exceeds 256 MiB aggregate decoded RGBA"));
    }
    let image = image::DynamicImage::from_decoder(decoder)
        .map_err(error)?
        .into_rgba8();
    if image.as_raw().len() as u64 != rgba {
        return Err(error("Invalid decoded image length"));
    }
    Ok(image)
}
impl AssetPack {
    /// Storage is untrusted too. Validate stored files, dictionaries and metadata on every load.
    pub fn validate(&self) -> Result<()> {
        validate_manifest(&self.manifest)?;
        if self.files.is_empty() && self.translations.is_empty() {
            return Err(error("Pack contains no supported images or translations"));
        }
        if self.files.len() + self.translations.len() + 1 > MAX_ENTRIES {
            return Err(error("Too many pack entries"));
        }
        let mut bytes = serde_json::to_vec(&self.manifest).map_err(error)?.len() as u64;
        let mut pixels = 0u64;
        for (path, data) in &self.files {
            let rel = path
                .strip_prefix("assets/")
                .ok_or_else(|| error("Invalid stored asset path"))?;
            if canonical_path(rel)? != rel || !image_path(rel) {
                return Err(error("Invalid stored image path"));
            }
            bytes = bytes
                .checked_add(data.len() as u64)
                .ok_or_else(|| error("Pack byte overflow"))?;
            pixels = pixels
                .checked_add(
                    decode_with_budget(data, MAX_PACK_RGBA - pixels)?
                        .as_raw()
                        .len() as u64,
                )
                .ok_or_else(|| error("Pack pixel overflow"))?;
            if pixels > MAX_PACK_RGBA {
                return Err(error("Pack exceeds 256 MiB aggregate decoded RGBA"));
            }
        }
        for (code, json) in &self.translations {
            if locale(&format!("lang/{code}.json")) != Some(code.as_str()) {
                return Err(error("Invalid locale code"));
            }
            let v: Value = serde_json::from_str(json).map_err(error)?;
            if !v.is_object() {
                return Err(error("Translation must contain an object"));
            }
            bytes = bytes
                .checked_add(json.len() as u64)
                .ok_or_else(|| error("Pack byte overflow"))?;
        }
        if bytes > MAX_EXTRACTED {
            return Err(error("Pack exceeds 60 MiB extracted"));
        }
        Ok(())
    }
}
/// Bound the central-directory allocation before zip constructs its entry vector.
fn admit_directory(bytes: &[u8]) -> Result<()> {
    let bounded = |offset: u64, length: u64, limit: usize| -> Result<()> {
        if offset
            .checked_add(length)
            .is_none_or(|end| end > limit as u64)
        {
            return Err(error("Malformed ZIP footer offset or length"));
        }
        Ok(())
    };
    let u16_at = |at: usize| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as u64;
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as u64;
    let u64_at = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    let mut found = false;
    // R125: zip2 retries earlier candidates. Admit every complete candidate, even in payloads.
    for (at, magic) in bytes.windows(4).enumerate() {
        match magic {
            b"PK\x05\x06" if at + 22 <= bytes.len() => {
                found = true;
                bounded(at as u64, 22 + u16_at(at + 20), bytes.len())?;
                let locator = at >= 20 && bytes.get(at - 20..at - 16) == Some(b"PK\x06\x07");
                for pos in [at + 8, at + 10] {
                    let count = u16_at(pos);
                    if count > MAX_ENTRIES as u64 && !(count == 65535 && locator) {
                        return Err(error("ZIP footer exceeds 1000 entries"));
                    }
                }
                let size = u32_at(at + 12);
                let offset = u32_at(at + 16);
                if size == u32::MAX as u64 || offset == u32::MAX as u64 {
                    if !locator {
                        return Err(error("Missing ZIP64 locator"));
                    }
                } else {
                    bounded(offset, size, at)?;
                }
            }
            b"PK\x06\x06" if at + 56 <= bytes.len() => {
                let size = u64_at(at + 4);
                if size < 44 {
                    return Err(error("Malformed ZIP64 record length"));
                }
                bounded(
                    at as u64,
                    size.checked_add(12)
                        .ok_or_else(|| error("ZIP64 length overflow"))?,
                    bytes.len(),
                )?;
                for pos in [at + 24, at + 32] {
                    if u64_at(pos) > MAX_ENTRIES as u64 {
                        return Err(error("ZIP64 footer exceeds 1000 entries"));
                    }
                }
                bounded(u64_at(at + 48), u64_at(at + 40), at)?;
            }
            b"PK\x06\x07" if at + 20 <= bytes.len() => {
                bounded(u64_at(at + 8), 56, at)?;
            }
            _ => {}
        }
    }
    if !found {
        return Err(error("Missing ZIP end record"));
    }
    Ok(())
}
/// zip2 deduplicates exact file names internally. Count original headers before trusting len().
fn admit_raw_entries(bytes: &[u8], start: u64, expected: usize) -> Result<()> {
    let mut at = usize::try_from(start).map_err(error)?;
    let mut count = 0;
    while bytes.get(at..at.saturating_add(4)) == Some(b"PK\x01\x02") {
        let header = bytes
            .get(at..at.saturating_add(46))
            .ok_or_else(|| error("Truncated central header"))?;
        let mut length = 46usize;
        for pos in [28, 30, 32] {
            length = length
                .checked_add(u16::from_le_bytes([header[pos], header[pos + 1]]) as usize)
                .ok_or_else(|| error("Central length overflow"))?;
        }
        at = at
            .checked_add(length)
            .filter(|&end| end <= bytes.len())
            .ok_or_else(|| error("Truncated central entry"))?;
        count += 1;
        if count > MAX_ENTRIES {
            return Err(error("ZIP exceeds 1000 raw entries"));
        }
    }
    if count != expected {
        return Err(error(
            "Duplicate ZIP file names or inconsistent central entry count",
        ));
    }
    Ok(())
}
pub fn read_pack(bytes: &[u8]) -> Result<AssetPack> {
    if bytes.len() > MAX_ZIP_BYTES {
        return Err(error("ZIP exceeds 25 MiB"));
    }
    admit_directory(bytes)?;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(error)?;
    admit_raw_entries(bytes, zip.central_directory_start(), zip.len())?;
    if zip.len() > MAX_ENTRIES {
        return Err(error("ZIP has more than 1000 entries"));
    }
    let mut paths = BTreeSet::new();
    let mut names = vec![];
    let mut manifest = None;
    let mut declared = 0u64;
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(error)?;
        let path = canonical_path(f.name())?;
        if !paths.insert(path.clone()) {
            return Err(error(format!("Duplicate canonical path: {path}")));
        }
        declared = declared
            .checked_add(f.size())
            .ok_or_else(|| error("Extracted size overflow"))?;
        if declared > MAX_EXTRACTED {
            return Err(error("ZIP exceeds 60 MiB extracted"));
        }
        if !f.is_dir()
            && path.rsplit('/').next() == Some("asset-pack.json")
            && manifest.replace((i, path.clone())).is_some()
        {
            return Err(error("Multiple asset-pack.json manifests"));
        }
        names.push((path, f.is_dir()));
    }
    let (manifest_index, manifest_path) =
        manifest.ok_or_else(|| error("Missing asset-pack.json"))?;
    let prefix = manifest_path.strip_suffix("asset-pack.json").unwrap();
    let mut actual = 0u64;
    let mut manifest_bytes = vec![];
    let mut files = BTreeMap::new();
    let mut translations = BTreeMap::new();
    for (i, (path, dir)) in names.into_iter().enumerate() {
        let f = zip.by_index(i).map_err(error)?;
        let mut data = vec![];
        f.take(MAX_EXTRACTED - actual + 1)
            .read_to_end(&mut data)
            .map_err(error)?;
        actual += data.len() as u64;
        if actual > MAX_EXTRACTED {
            return Err(error("ZIP exceeds actual 60 MiB extracted"));
        }
        if i == manifest_index {
            manifest_bytes = data;
            continue;
        }
        if dir {
            continue;
        }
        let Some(relative) = path.strip_prefix(prefix) else {
            continue;
        };
        if image_path(relative) {
            files.insert(format!("assets/{relative}"), data);
        } else if let Some(code) = locale(relative) {
            let value: Value = serde_json::from_slice(json_bytes(&data)).map_err(error)?;
            if !value.is_object() {
                return Err(error("Translation must contain an object"));
            }
            translations.insert(
                code.to_owned(),
                String::from_utf8(json_bytes(&data).to_vec()).map_err(error)?,
            );
        }
    }
    let manifest: Value = serde_json::from_slice(json_bytes(&manifest_bytes)).map_err(error)?;
    validate_manifest(&manifest)?;
    let pack = AssetPack {
        id: manifest
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(manifest["name"].as_str().unwrap())
            .to_owned(),
        manifest,
        files,
        translations,
    };
    pack.validate()?;
    Ok(pack)
}
