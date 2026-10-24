//! Shared by build.rs (the native build's embedded faces) and `kit::fonts` (the faces the web
//! build fetches, R208), so both builds hint a bundled face alike.

/// Chromium rasters text with Skia's `kSlight` hinting, i.e. FreeType `FT_LOAD_TARGET_LIGHT`,
/// which autohints every TrueType face (light mode never runs a font's instructions); egui's
/// skrifa autohints only faces without instructions, so this empties `fpgm`, `prep`, `cvt ` and
/// `maxp.maxSizeOfInstructions` (lengths only: the table directory stays sorted). A truncated
/// table directory leaves the rest of the bytes as they are.
pub fn without_instructions(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let be32 = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let tables = bytes
        .get(4..6)
        .map_or(0, |b| u16::from_be_bytes([b[0], b[1]]));
    for rec in (0..tables as usize).map(|i| 12 + 16 * i) {
        let (Some(tag), Some(offset)) = (bytes.get(rec..rec + 4), be32(rec + 8)) else {
            break;
        };
        let offset = offset as usize;
        let zero = match tag {
            b"fpgm" | b"prep" | b"cvt " => rec + 12..rec + 16,
            // maxp 1.0 `maxSizeOfInstructions`.
            b"maxp" if be32(offset) == Some(0x0001_0000) => offset + 26..offset + 28,
            _ => continue,
        };
        if let Some(field) = out.get_mut(zero) {
            field.fill(0);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn truncated_directory_is_left_alone() {
        // Nine records claimed; the `fpgm` one stops before its length field.
        let mut bytes = vec![0, 1, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0];
        bytes.extend(b"fpgm\0\0\0\0\0\0\0\0");
        assert_eq!(super::without_instructions(&bytes), bytes);
    }
}
