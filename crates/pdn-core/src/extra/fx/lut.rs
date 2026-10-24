//! Ported from PhotoCraft crates/cms/src/lutfile.rs and crates/compose/src/adjust.rs (`lut3d_sample`, `bayer4`) @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: `write_cube` and its round-trip tests dropped (nothing exports a LUT); `lut3d_sample`/`bayer4` moved here from the compose evaluator; `LutFile::apply` added (Color Lookup's per-pixel step), carrying the input domain where upstream rejects a non-default one; `admit` added (the File source's extension check); a file over 128 MiB, or a table past 129³ entries, is refused; parse errors quote at most 40 characters of a line.
//!
//! 3D LUT files for Color Lookup: `.cube` (Adobe/Resolve), `.3dl` (Autodesk Lustre / Flame) and
//! `.look` (SpeedGrade XML), plus the generated (CC0) looks. Every table is a [`LutFile`]: `size`³
//! RGB triplets in 0..=1, **red varying fastest** (`((b·size + g)·size + r)·3`).

/// A parsed 3D LUT.
#[derive(Clone, Debug, PartialEq)]
pub struct LutFile {
    pub title: String,
    pub size: usize,
    /// `size`³ × 3 values, red fastest.
    pub data: Vec<f32>,
    /// The input domain the table expects (`.cube` `DOMAIN_MIN`/`DOMAIN_MAX`,
    /// `LUT_3D_INPUT_RANGE`): an input `c` samples the table at `(c - min) / (max - min)`,
    /// saturating outside. `[0, 1]` is the identity default; `apply` carries any other.
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
}

/// Largest edge length accepted (64³ is Photoshop's practical maximum; 129 leaves headroom while
/// keeping the GPU table within texture limits).
pub const MAX_SIZE: usize = 129;

/// Largest file read: a 129³ `.cube`, the largest valid table, is about 70–80 MB.
pub const MAX_FILE_BYTES: usize = 128 << 20;
/// The refusal of a file over [`MAX_FILE_BYTES`], made before parsing.
pub const TOO_BIG: &str = "The file is larger than 128 MiB, too large for a 3D LUT.";
/// The refusal of a table with more entries than a [`MAX_SIZE`]³ one holds.
pub const TOO_MANY: &str = "The file holds more entries than a 129³ LUT.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LutError(pub String);

impl std::fmt::Display for LutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LutError {}

fn err<T>(msg: impl Into<String>) -> Result<T, LutError> {
    Err(LutError(msg.into()))
}

/// `line` cut to 40 characters, for a message: a binary file's "line" can be any length.
fn quote(line: &str) -> &str {
    line.char_indices()
        .nth(40)
        .map_or(line, |(i, _)| &line[..i])
}

fn checked_data_len(size: usize) -> Result<(usize, usize), LutError> {
    if !(2..=MAX_SIZE).contains(&size) {
        return err(format!("unsupported LUT size {size}"));
    }
    let entries = size
        .checked_pow(3)
        .ok_or_else(|| LutError(format!("unsupported LUT size {size}")))?;
    let data_len = entries
        .checked_mul(3)
        .ok_or_else(|| LutError(format!("unsupported LUT size {size}")))?;
    Ok((entries, data_len))
}

impl LutFile {
    /// The identity table of edge `size`.
    pub fn identity(size: usize) -> Self {
        Self::from_fn("Identity", size, |c| c)
    }

    /// Samples `f` on an `size`³ grid.
    pub fn from_fn(title: &str, size: usize, f: impl Fn([f32; 3]) -> [f32; 3]) -> Self {
        let m = (size.max(2) - 1) as f32;
        let mut data = Vec::with_capacity(size * size * size * 3);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let o = f([r as f32 / m, g as f32 / m, b as f32 / m]);
                    data.extend(o.map(|v| v.clamp(0.0, 1.0)));
                }
            }
        }
        LutFile {
            title: title.to_string(),
            size,
            data,
            domain_min: [0.0; 3],
            domain_max: [1.0; 3],
        }
    }

    /// Whether the table expects plain 0..1 inputs (the common case; Photoshop's default).
    pub fn domain_is_default(&self) -> bool {
        self.domain_min == [0.0, 0.0, 0.0] && self.domain_max == [1.0, 1.0, 1.0]
    }

    fn check(self) -> Result<Self, LutError> {
        let (entries, data_len) = checked_data_len(self.size)?;
        if self.data.len() != data_len {
            return err(format!(
                "expected {entries} entries, found {}",
                self.data.len() / 3
            ));
        }
        if self.data.iter().any(|v| !v.is_finite()) {
            return err("non-finite LUT value");
        }
        Ok(self)
    }
}

/// The file types Color Lookup reads, as its chooser lists them.
pub const EXTENSIONS: [&str; 3] = ["cube", "3dl", "look"];

/// Whether `name`'s extension (any case) is one of [`EXTENSIONS`], judged before any bytes are
/// read; otherwise "‹ext› is not supported.", worded as Fill From File's refusal.
pub fn admit(name: &str) -> Result<(), String> {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let ext = file.rfind('.').map_or("", |i| &file[i..]);
    if EXTENSIONS
        .iter()
        .any(|e| ext[ext.len().min(1)..].eq_ignore_ascii_case(e))
    {
        Ok(())
    } else {
        Err(format!("{ext} is not supported."))
    }
}

/// Parses a LUT, picking the format from `file_name`'s extension (falls back to sniffing).
pub fn parse(file_name: &str, bytes: &[u8]) -> Result<LutFile, LutError> {
    if bytes.len() > MAX_FILE_BYTES {
        return err(TOO_BIG);
    }
    let lower = file_name.to_ascii_lowercase();
    let text = String::from_utf8_lossy(bytes);
    if lower.ends_with(".3dl") {
        parse_3dl(&text)
    } else if lower.ends_with(".look") || text.trim_start().starts_with('<') {
        parse_look(&text)
    } else {
        parse_cube(&text)
    }
}

/// Adobe / Resolve `.cube`. 1D LUTs (`LUT_1D_SIZE`) are expanded to a 33³ table.
pub fn parse_cube(text: &str) -> Result<LutFile, LutError> {
    let mut title = String::new();
    let (mut size3, mut size1) = (0usize, 0usize);
    let (mut dmin, mut dmax) = ([0.0f32; 3], [1.0f32; 3]);
    let mut rows: Vec<f32> = Vec::new();
    for line in text.lines() {
        // A '#' inside a quoted TITLE is part of the title, not a comment.
        let raw = line.trim_start();
        let skip = if raw.starts_with("TITLE") {
            raw.find('"')
                .and_then(|q| raw[q + 1..].find('"').map(|e| q + e + 2))
        } else {
            None
        }
        .unwrap_or(0);
        let line = match raw[skip..].find('#') {
            Some(i) => &raw[..skip + i],
            None => raw,
        }
        .trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        let key = it.next().unwrap_or("");
        let nums = |it: std::str::SplitWhitespace<'_>| -> Result<[f32; 3], LutError> {
            let v: Vec<f32> = it.filter_map(|s| s.parse().ok()).collect();
            if v.len() == 3 {
                Ok([v[0], v[1], v[2]])
            } else {
                err(format!("bad line `{}`", quote(line)))
            }
        };
        match key {
            "TITLE" => title = line[5..].trim().trim_matches('"').to_string(),
            "LUT_3D_SIZE" => {
                size3 = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| LutError("bad LUT_3D_SIZE".into()))?
            }
            "LUT_1D_SIZE" => {
                size1 = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| LutError("bad LUT_1D_SIZE".into()))?
            }
            "DOMAIN_MIN" => dmin = nums(it)?,
            "DOMAIN_MAX" => dmax = nums(it)?,
            "LUT_3D_INPUT_RANGE" | "LUT_1D_INPUT_RANGE" => {
                let v: Vec<f32> = it.filter_map(|s| s.parse().ok()).collect();
                if v.len() == 2 {
                    dmin = [v[0]; 3];
                    dmax = [v[1]; 3];
                }
            }
            k if k
                .starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+' || c == '.') =>
            {
                let mut v = vec![
                    k.parse::<f32>()
                        .map_err(|_| LutError(format!("bad number in `{}`", quote(line))))?,
                ];
                v.extend(it.filter_map(|s| s.parse::<f32>().ok()));
                if v.len() != 3 {
                    return err(format!("bad data line `{}`", quote(line)));
                }
                if rows.len() >= MAX_SIZE.pow(3) * 3 {
                    return err(TOO_MANY);
                }
                rows.extend(v);
            }
            // Unknown keywords (e.g. Resolve's LUT_IN_VIDEO_RANGE) are ignored, as the spec allows.
            _ => {}
        }
    }
    let domain_ok = (0..3).all(|k| dmin[k] < dmax[k] && dmin[k].is_finite() && dmax[k].is_finite());
    if (size3 > 0 || size1 >= 2) && !domain_ok {
        return err(format!(
            "DOMAIN_MIN {:?} must lie below DOMAIN_MAX {:?}",
            dmin, dmax
        ));
    }
    if size3 > 0 {
        return LutFile {
            title,
            size: size3,
            data: rows,
            domain_min: dmin,
            domain_max: dmax,
        }
        .check();
    }
    if size1 >= 2 {
        let expected_rows = size1
            .checked_mul(3)
            .ok_or_else(|| LutError(format!("unsupported 1D LUT size {size1}")))?;
        if rows.len() != expected_rows {
            return err("no LUT_3D_SIZE in .cube file");
        }
        // The 1D table is resampled onto a 0..1 cube anyway, so its input domain
        // (`DOMAIN_MIN`/`MAX`, `LUT_1D_INPUT_RANGE`) is baked into the resample: an input `v`
        // reads the table at `(v - min) / (max - min)`, saturating outside.
        let curve = |ch: usize, v: f32| {
            let t = (v - dmin[ch]) / (dmax[ch] - dmin[ch]);
            let x = if t.is_finite() {
                t.clamp(0.0, 1.0)
            } else {
                0.0
            } * (size1 - 1) as f32;
            let i = (x.floor() as usize).min(size1 - 2);
            let f = x - i as f32;
            rows[i * 3 + ch] * (1.0 - f) + rows[(i + 1) * 3 + ch] * f
        };
        let mut l = LutFile::from_fn(&title, 33, |c| {
            [curve(0, c[0]), curve(1, c[1]), curve(2, c[2])]
        });
        l.title = title;
        return l.check();
    }
    err("no LUT_3D_SIZE in .cube file")
}

/// Lustre `.3dl`: an optional shaper line of input values, then integer RGB rows with **blue**
/// varying fastest; the output bit depth is inferred from the largest value.
pub fn parse_3dl(text: &str) -> Result<LutFile, LutError> {
    let mut rows: Vec<[f64; 3]> = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with(|c: char| c.is_ascii_alphabetic()) {
            continue;
        }
        let v: Vec<f64> = line
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        if v.len() == 3 {
            if rows.len() >= MAX_SIZE.pow(3) {
                return err(TOO_MANY);
            }
            rows.push([v[0], v[1], v[2]]);
        }
        // Lines with more than three numbers are the input shaper; it is assumed linear.
    }
    let n = (rows.len() as f64).cbrt().round() as usize;
    if n < 2 || n * n * n != rows.len() {
        return err(format!("{} rows is not a cube", rows.len()));
    }
    let max = rows.iter().flatten().fold(0.0f64, |a, &b| a.max(b));
    let scale = if max <= 1.0 {
        1.0
    } else {
        [1023.0, 4095.0, 16383.0, 65535.0]
            .into_iter()
            .find(|&s| max <= s)
            .unwrap_or(max)
    };
    let mut data = vec![0.0f32; n * n * n * 3];
    for (i, row) in rows.iter().enumerate() {
        let (r, g, b) = (i / (n * n), (i / n) % n, i % n);
        let at = ((b * n + g) * n + r) * 3;
        for k in 0..3 {
            data[at + k] = (row[k] / scale) as f32;
        }
    }
    LutFile {
        title: String::new(),
        size: n,
        data,
        domain_min: [0.0; 3],
        domain_max: [1.0; 3],
    }
    .check()
}

/// SpeedGrade `.look`: `<size>` and a hex `<data>` string of little-endian float32 RGB triplets
/// (red fastest).
pub fn parse_look(text: &str) -> Result<LutFile, LutError> {
    let tag = |name: &str| -> Option<String> {
        let open = format!("<{name}>");
        let start = text.find(&open)? + open.len();
        let end = text[start..].find(&format!("</{name}>"))? + start;
        Some(text[start..end].trim().trim_matches('"').trim().to_string())
    };
    let size: usize = tag("size")
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| LutError("no <size> in .look".into()))?;
    let (entries, _) = checked_data_len(size)?;
    let hex: Vec<u8> = tag("data")
        .ok_or_else(|| LutError("no <data> in .look".into()))?
        .bytes()
        .filter(u8::is_ascii_hexdigit)
        .collect();
    let nib = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
    let bytes: Vec<u8> = hex
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| nib(p[0]) << 4 | nib(p[1]))
        .collect();
    let floats: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    let rgba_entries = entries
        .checked_mul(4)
        .ok_or_else(|| LutError(format!("unsupported LUT size {size}")))?;
    let data = if floats.len() == rgba_entries {
        floats
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|c| [c[0], c[1], c[2]])
            .collect()
    } else {
        floats
    };
    LutFile {
        title: tag("title").unwrap_or_default(),
        size,
        data,
        domain_min: [0.0; 3],
        domain_max: [1.0; 3],
    }
    .check()
}

fn luma(c: [f32; 3]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Generated looks shipped with PhotoCraft (procedural, CC0): `(id, label)`.
pub const BUILTIN: [(&str, &str); 8] = [
    ("warm", "Warm Filter"),
    ("cool", "Cool Filter"),
    ("tealOrange", "Teal & Orange"),
    ("bleachBypass", "Bleach Bypass"),
    ("fadedFilm", "Faded Film"),
    ("dayForNight", "Day for Night"),
    ("monoContrast", "Mono High Contrast"),
    ("crossProcess", "Cross Process"),
];

/// A built-in look by id (33³), or None.
pub fn builtin(id: &str) -> Option<LutFile> {
    let label = BUILTIN.iter().find(|b| b.0 == id)?.1;
    let f: Box<dyn Fn([f32; 3]) -> [f32; 3]> = match id {
        "warm" => Box::new(|c: [f32; 3]| [c[0] * 0.92 + 0.08, c[1] * 0.97 + 0.02, c[2] * 0.85]),
        "cool" => Box::new(|c: [f32; 3]| [c[0] * 0.86, c[1] * 0.97 + 0.01, c[2] * 0.9 + 0.1]),
        "tealOrange" => Box::new(|c: [f32; 3]| {
            // Shadows toward teal, highlights toward orange, skin-ish midtones kept.
            let l = luma(c);
            let t = smooth(l);
            let teal = [0.0, 0.5, 0.55];
            let orange = [1.0, 0.62, 0.3];
            std::array::from_fn(|k| {
                let tint = teal[k] * (1.0 - t) + orange[k] * t;
                c[k] * 0.75 + (tint * l * 1.1).min(1.0) * 0.25 + (smooth(c[k]) - c[k]) * 0.3
            })
        }),
        "bleachBypass" => Box::new(|c: [f32; 3]| {
            let l = luma(c);
            let s = smooth(l);
            std::array::from_fn(|k| {
                let desat = c[k] * 0.45 + l * 0.55;
                desat * 0.5 + s * 0.5
            })
        }),
        "fadedFilm" => Box::new(|c: [f32; 3]| {
            let l = luma(c);
            let tint = [1.0, 0.96, 0.88];
            std::array::from_fn(|k| (0.08 + (c[k] * 0.8 + l * 0.2) * 0.84) * tint[k] + 0.02)
        }),
        "dayForNight" => Box::new(|c: [f32; 3]| {
            let l = luma(c);
            [l * 0.35, l * 0.45 + c[1] * 0.05, l * 0.65 + c[2] * 0.15]
        }),
        "monoContrast" => Box::new(|c: [f32; 3]| [smooth(smooth(luma(c))); 3]),
        _ /* crossProcess */ => Box::new(|c: [f32; 3]| [smooth(c[0]) * 1.05, c[1] * 1.1 - 0.03, c[2] * 0.7 + 0.15]),
    };
    Some(LutFile::from_fn(label, 33, f))
}

/// Ordered-dither offset in -0.5..0.5 (4×4 Bayer matrix) for document pixel (x, y).
pub fn bayer4(x: i32, y: i32) -> f32 {
    const M: [f32; 16] = [
        0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0,
    ];
    (M[((y & 3) * 4 + (x & 3)) as usize] + 0.5) / 16.0 - 0.5
}

/// Samples a 3D LUT (`n`³ RGB triplets, red fastest) at `c`, trilinear or tetrahedral.
pub fn lut3d_sample(lut: &[f32], n: usize, c: [f32; 3], tetrahedral: bool) -> [f32; 3] {
    // A degenerate or short table can't be sampled: leave the colour alone.
    if n < 2
        || n.checked_pow(3)
            .and_then(|c| c.checked_mul(3))
            .is_none_or(|len| lut.len() < len)
    {
        return c;
    }
    let m = (n - 1) as f32;
    let pos = c.map(|v| v.clamp(0.0, 1.0) * m);
    let i0 = pos.map(|p| (p.floor() as usize).min(n - 2));
    let f: [f32; 3] = std::array::from_fn(|k| pos[k] - i0[k] as f32);
    let at = |dr: usize, dg: usize, db: usize| -> [f32; 3] {
        let idx = (((i0[2] + db) * n + i0[1] + dg) * n + i0[0] + dr) * 3;
        [lut[idx], lut[idx + 1], lut[idx + 2]]
    };
    let mix = |w: &[(f32, [f32; 3])]| -> [f32; 3] {
        std::array::from_fn(|k| w.iter().map(|(a, v)| a * v[k]).sum())
    };
    let (fr, fg, fb) = (f[0], f[1], f[2]);
    if tetrahedral {
        let (c000, c111) = (at(0, 0, 0), at(1, 1, 1));
        if fr > fg {
            if fg > fb {
                mix(&[
                    (1.0 - fr, c000),
                    (fr - fg, at(1, 0, 0)),
                    (fg - fb, at(1, 1, 0)),
                    (fb, c111),
                ])
            } else if fr > fb {
                mix(&[
                    (1.0 - fr, c000),
                    (fr - fb, at(1, 0, 0)),
                    (fb - fg, at(1, 0, 1)),
                    (fg, c111),
                ])
            } else {
                mix(&[
                    (1.0 - fb, c000),
                    (fb - fr, at(0, 0, 1)),
                    (fr - fg, at(1, 0, 1)),
                    (fg, c111),
                ])
            }
        } else if fb > fg {
            mix(&[
                (1.0 - fb, c000),
                (fb - fg, at(0, 0, 1)),
                (fg - fr, at(0, 1, 1)),
                (fr, c111),
            ])
        } else if fb > fr {
            mix(&[
                (1.0 - fg, c000),
                (fg - fb, at(0, 1, 0)),
                (fb - fr, at(0, 1, 1)),
                (fr, c111),
            ])
        } else {
            mix(&[
                (1.0 - fg, c000),
                (fg - fr, at(0, 1, 0)),
                (fr - fb, at(1, 1, 0)),
                (fb, c111),
            ])
        }
    } else {
        let lerp = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
            std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t)
        };
        let c00 = lerp(at(0, 0, 0), at(1, 0, 0), fr);
        let c10 = lerp(at(0, 1, 0), at(1, 1, 0), fr);
        let c01 = lerp(at(0, 0, 1), at(1, 0, 1), fr);
        let c11 = lerp(at(0, 1, 1), at(1, 1, 1), fr);
        lerp(lerp(c00, c10, fg), lerp(c01, c11, fg), fb)
    }
}

impl LutFile {
    /// `buf` (interleaved straight RGBA, 0–1) through the table, read over its input domain;
    /// alpha untouched. `dither` adds a 4×4 ordered offset of up to half an 8-bit step to hide
    /// banding.
    pub fn apply(&self, buf: &mut [f32], w: usize, tetrahedral: bool, dither: bool) {
        let (lo, hi) = (self.domain_min, self.domain_max);
        for (i, p) in buf.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            // The default [0, 1] gives the input back bit for bit.
            let c = std::array::from_fn(|k| (p[k] - lo[k]) / (hi[k] - lo[k]));
            let mut o = lut3d_sample(&self.data, self.size, c, tetrahedral);
            if dither {
                let d = bayer4((i % w) as i32, (i / w) as i32) / 255.0;
                o = o.map(|v| (v + d).clamp(0.0, 1.0));
            }
            p[..3].copy_from_slice(&o);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUBE_ROWS: &str = "0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";

    /// A `.cube` of the identity cube, red varying fastest, parses to `LutFile::identity`.
    #[test]
    fn identity_cube_is_identity() {
        let text = format!("TITLE \"Id\"\nLUT_3D_SIZE 2\n{CUBE_ROWS}");
        let l = parse_cube(&text).unwrap();
        assert_eq!((l.size, l.title.as_str()), (2, "Id"));
        assert_eq!(l.data, LutFile::identity(2).data);
        // Applying it changes no colour.
        let mut buf = [0.2, 0.5, 0.9, 0.5, 1.0, 0.0, 0.3, 1.0];
        let before = buf;
        l.apply(&mut buf, 2, false, false);
        assert!(buf.iter().zip(before).all(|(a, b)| (a - b).abs() < 1e-6));
    }

    /// The three formats: `.cube` (red fastest), `.3dl` (blue fastest, 12-bit) and `.look` (hex
    /// float32) all give the identity table.
    #[test]
    fn parses_cube_3dl_look() {
        let id = LutFile::identity(2);
        let cube = parse("a.cube", format!("LUT_3D_SIZE 2\n{CUBE_ROWS}").as_bytes()).unwrap();
        assert_eq!(cube.data, id.data);
        let mut text = String::from("0 1023\n");
        for r in 0..2 {
            for g in 0..2 {
                for b in 0..2 {
                    text.push_str(&format!("{} {} {}\n", r * 4095, g * 4095, b * 4095));
                }
            }
        }
        assert_eq!(parse("a.3dl", text.as_bytes()).unwrap().data, id.data);
        let hex: String = id
            .data
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .map(|b| format!("{b:02X}"))
            .collect();
        let xml = format!(
            "<?xml version=\"1.0\"?><look><LUT><size>\"2\"</size><data>\"{hex}\"</data></LUT></look>"
        );
        assert_eq!(parse("a.look", xml.as_bytes()).unwrap().data, id.data);
    }

    #[test]
    fn cube_title_with_hash_is_kept() {
        let t = "TITLE \"A #b\" # note\nLUT_3D_SIZE 2 # c\n# comment\n";
        let rows = "0 0 0 # x\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
        assert_eq!(parse_cube(&format!("{t}{rows}")).unwrap().title, "A #b");
    }

    #[test]
    fn cube_domain_is_kept_and_validated() {
        let base = "TITLE \"t\"\nLUT_3D_SIZE 2\n";
        let rows = "0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n";
        assert!(
            parse_cube(&format!("{base}{rows}"))
                .unwrap()
                .domain_is_default()
        );
        let d = parse_cube(&format!(
            "DOMAIN_MIN 0.1 0.2 0.3\nDOMAIN_MAX 0.9 0.8 0.7\n{base}{rows}"
        ))
        .unwrap();
        assert_eq!(d.domain_min, [0.1, 0.2, 0.3]);
        assert_eq!(d.domain_max, [0.9, 0.8, 0.7]);
        let e = parse_cube(&format!(
            "DOMAIN_MIN 0.5 0.5 0.5\nDOMAIN_MAX 0.5 1 1\n{base}{rows}"
        ))
        .unwrap_err();
        assert!(e.0.contains("below DOMAIN_MAX"), "{e}");
    }

    #[test]
    fn cube_1d_domain_is_baked_into_the_resample() {
        let table = "LUT_1D_SIZE 2\nLUT_1D_INPUT_RANGE 0.25 0.75\n0 0 0\n1 1 1\n";
        let l = parse_cube(table).unwrap();
        assert!(l.domain_is_default());
        let at = |r: usize| l.data[r * 3];
        assert!(at(0).abs() < 1e-6 && (at(32) - 1.0).abs() < 1e-6);
        assert!((at(16) - 0.5).abs() < 1e-5 && at(8).abs() < 1e-5 && (at(24) - 1.0).abs() < 1e-5);
        let e = parse_cube("LUT_1D_SIZE 2\nLUT_1D_INPUT_RANGE 1 1\n0 0 0\n1 1 1\n").unwrap_err();
        assert!(e.0.contains("below DOMAIN_MAX"), "{e}");
    }

    #[test]
    fn cube_1d_expands() {
        let l = parse_cube("LUT_1D_SIZE 2\n0 0 0\n0.5 1 1\n").unwrap();
        assert_eq!(l.size, 33);
        let last = &l.data[l.data.len() - 3..];
        assert!((last[0] - 0.5).abs() < 1e-6 && (last[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn overflowing_sizes_are_rejected() {
        assert!(parse_cube(&format!("LUT_1D_SIZE {}\n", usize::MAX)).is_err());
        let text = format!("<look><size>{}</size><data>00</data></look>", usize::MAX);
        assert!(parse_look(&text).is_err());
    }

    #[test]
    fn malformed_rejected() {
        assert!(parse_cube("LUT_3D_SIZE 2\n0 0 0\n").is_err());
        assert!(parse_cube("garbage").is_err());
        assert!(parse_cube("LUT_3D_SIZE 99999\n").is_err());
        assert!(parse_3dl("1 2 3\n4 5 6\n").is_err());
        assert!(parse_look("<look></look>").is_err());
    }

    #[test]
    fn builtins_exist_and_are_in_range() {
        for (id, _) in BUILTIN {
            let l = builtin(id).unwrap();
            assert_eq!(l.data.len(), 33 * 33 * 33 * 3);
            assert!(l.data.iter().all(|v| (0.0..=1.0).contains(v)), "{id}");
        }
        assert!(builtin("nope").is_none());
    }

    /// Both interpolations reproduce a table's corners exactly and agree on the identity.
    #[test]
    fn interpolations_agree_on_identity() {
        let l = LutFile::identity(5);
        for c in [
            [0.1, 0.7, 0.3],
            [0.9, 0.2, 0.2],
            [0.5, 0.5, 0.5],
            [1.0, 0.0, 1.0],
        ] {
            for tet in [false, true] {
                let o = lut3d_sample(&l.data, 5, c, tet);
                assert!(
                    o.iter().zip(c).all(|(a, b)| (a - b).abs() < 1e-5),
                    "{c:?} {tet}"
                );
            }
        }
    }

    /// PC29: a table's input domain is carried through. With DOMAIN_MAX 2 the identity table is
    /// read at half the input, so 0.5 gives 0.25 and 1 gives 0.5.
    #[test]
    fn apply_carries_the_input_domain() {
        let text = format!("LUT_3D_SIZE 2\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n{CUBE_ROWS}");
        let l = parse_cube(&text).unwrap();
        assert!(!l.domain_is_default());
        let mut buf = [0.5, 1.0, 0.0, 1.0];
        l.apply(&mut buf, 1, false, false);
        assert!(
            (buf[0] - 0.25).abs() < 1e-6 && (buf[1] - 0.5).abs() < 1e-6 && buf[2] == 0.0,
            "{buf:?}"
        );
    }

    /// Color Lookup admits its three types by extension, in any case; anything else is refused
    /// as Fill From File words it.
    #[test]
    fn admit_takes_the_lut_extensions() {
        for name in ["a.cube", "dir/B.3DL", "c.Look"] {
            assert_eq!(admit(name), Ok(()), "{name}");
        }
        assert_eq!(admit("x.txt"), Err(".txt is not supported.".into()));
        assert_eq!(admit("plain"), Err(" is not supported.".into()));
    }

    /// N2: a file over 128 MiB is refused before it is parsed, and a table stops once it has more
    /// entries than a 129³ one holds.
    #[test]
    fn oversized_files_and_tables_are_refused() {
        let e = parse("huge.cube", &vec![b'0'; MAX_FILE_BYTES + 1]).unwrap_err();
        assert_eq!(e.0, TOO_BIG);
        let rows = "0 0 0\n".repeat(MAX_SIZE.pow(3) + 1);
        assert_eq!(parse_cube(&rows).unwrap_err().0, TOO_MANY);
        assert_eq!(parse_3dl(&rows).unwrap_err().0, TOO_MANY);
    }

    /// A bad line is quoted at most 40 characters long, so a binary file's error stays short.
    #[test]
    fn errors_quote_a_short_line() {
        let text = format!("LUT_3D_SIZE 2\n{} x\n", "7".repeat(500));
        let e = parse_cube(&text).unwrap_err();
        assert!(e.0.len() < 80, "{e}");
    }
}
