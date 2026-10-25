//! The Windows .exe's resources, written by build.rs for Windows MSVC targets: the
//! `assets/app-icon.ico` images as RT_ICON entries under one RT_GROUP_ICON (Explorer's icon for
//! the file), and an RT_VERSION block (the Details tab, Task Manager's name). The output is a
//! `.res` file, which link.exe and lld-link take as an input.
//! Formats: Microsoft's "Resource File Formats" (RESOURCEHEADER), ICONDIR and GRPICONDIR, and
//! VS_VERSIONINFO.

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
const EN_US: u16 = 0x0409;

/// One image of an .ico file.
pub struct IcoImage<'a> {
    /// The head of its ICONDIRENTRY: width, height, colours, reserved, planes, bit count and byte
    /// count, which a GRPICONDIRENTRY repeats.
    pub entry: [u8; 12],
    /// A DIB or a PNG.
    pub data: &'a [u8],
}

pub fn ico_images(ico: &[u8]) -> Result<Vec<IcoImage<'_>>, String> {
    let u16_at = |at: usize| {
        ico.get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let u32_at = |at: usize| {
        ico.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if u16_at(0) != Some(0) || u16_at(2) != Some(1) {
        return Err("not an .ico file".into());
    }
    let count = u16_at(4).unwrap_or(0) as usize;
    (0..count)
        .map(|i| {
            let at = 6 + 16 * i;
            let (Some(entry), Some(len), Some(offset)) =
                (ico.get(at..at + 12), u32_at(at + 8), u32_at(at + 12))
            else {
                return Err("truncated .ico directory".into());
            };
            let data = ico
                .get(offset as usize..)
                .and_then(|rest| rest.get(..len as usize))
                .ok_or("truncated .ico image")?;
            Ok(IcoImage {
                entry: entry.try_into().unwrap(),
                data,
            })
        })
        .collect()
}

/// A `.res` file with `ico`'s images as the program icon and a version block for `version`
/// (`CARGO_PKG_VERSION`).
pub fn res_file(ico: &[u8], version: &str) -> Result<Vec<u8>, String> {
    let images = ico_images(ico)?;
    // Every .res file opens with an empty resource: type 0, name 0, no data.
    let mut out: Vec<u8> = [0u32, 32, 0xFFFF, 0xFFFF, 0, 0, 0, 0]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    let mut group = [0u16, 1, images.len() as u16]
        .map(u16::to_le_bytes)
        .concat();
    for (id, image) in (1u16..).zip(&images) {
        resource(&mut out, RT_ICON, id, 0x1010, image.data);
        group.extend(image.entry);
        group.extend(id.to_le_bytes());
    }
    // Memory flags as rc.exe writes them (moveable, pure, discardable); Windows ignores them.
    resource(&mut out, RT_GROUP_ICON, 1, 0x1030, &group);
    resource(&mut out, RT_VERSION, 1, 0x0030, &version_info(version));
    Ok(out)
}

/// A RESOURCEHEADER with an ordinal type and name, then the data, padded to 4 bytes.
fn resource(out: &mut Vec<u8>, kind: u16, id: u16, flags: u16, data: &[u8]) {
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(32u32.to_le_bytes());
    for word in [0xFFFF, kind, 0xFFFF, id] {
        out.extend(word.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes()); // DataVersion
    out.extend(flags.to_le_bytes());
    out.extend(EN_US.to_le_bytes());
    out.extend([0; 8]); // Version, Characteristics
    out.extend(data);
    pad(out);
}

fn pad(out: &mut Vec<u8>) {
    out.resize(out.len().next_multiple_of(4), 0);
}

fn utf16z(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// VS_VERSIONINFO: the numeric versions (`1.2.3-rc.1` is 1.2.3.0) and the US-English strings.
fn version_info(version: &str) -> Vec<u8> {
    let mut parts = version.split(['-', '+']).next().unwrap_or("").split('.');
    let mut part = || {
        u32::from(
            parts
                .next()
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(0),
        )
    };
    let (major, minor, patch, build) = (part(), part(), part(), part());
    let (ms, ls) = (major << 16 | minor, patch << 16 | build);
    // VS_FIXEDFILEINFO: signature, structure version, file and product versions, every flag
    // valid and none set, VOS_NT_WINDOWS32, VFT_APP, no subtype or date.
    let fixed: Vec<u8> = [
        0xFEEF04BD,
        0x0001_0000,
        ms,
        ls,
        ms,
        ls,
        0x3F,
        0,
        0x0004_0004,
        1,
        0,
        0,
        0,
    ]
    .iter()
    .flat_map(|w: &u32| w.to_le_bytes())
    .collect();
    let strings: Vec<Vec<u8>> = [
        ("FileDescription", "paint.rs"),
        ("FileVersion", version),
        ("LegalCopyright", "GPL-3.0-or-later"),
        ("OriginalFilename", "paintrs.exe"),
        ("ProductName", "paint.rs"),
        ("ProductVersion", version),
    ]
    .iter()
    .map(|(key, value)| block(key, 1, &utf16z(value), &[]))
    .collect();
    // 0409 04B0: US English, UTF-16.
    let table = block("040904B0", 1, &[], &strings);
    let translation = block("Translation", 0, &[0x09, 0x04, 0xB0, 0x04], &[]);
    block(
        "VS_VERSION_INFO",
        0,
        &fixed,
        &[
            block("StringFileInfo", 1, &[], &[table]),
            block("VarFileInfo", 1, &[], &[translation]),
        ],
    )
}

/// A version block: wLength, wValueLength (bytes for binary, UTF-16 units for text), wType
/// (1 text), the key, then the value and each child at a 4-byte boundary.
fn block(key: &str, kind: u16, value: &[u8], children: &[Vec<u8>]) -> Vec<u8> {
    let value_len = if kind == 1 {
        value.len() / 2
    } else {
        value.len()
    };
    let mut out = [0, value_len as u16, kind].map(u16::to_le_bytes).concat();
    out.extend(utf16z(key));
    pad(&mut out);
    out.extend(value);
    for child in children {
        pad(&mut out);
        out.extend(child);
    }
    let len = out.len() as u16;
    out[..2].copy_from_slice(&len.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([b[at], b[at + 1]])
    }
    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    /// (type, name, language, data) of each resource; ordinal types and names only.
    fn resources(res: &[u8]) -> Vec<(u16, u16, u16, &[u8])> {
        let mut out = vec![];
        let mut at = 0;
        while at < res.len() {
            let (size, header) = (u32_at(res, at) as usize, u32_at(res, at + 4) as usize);
            assert_eq!(header, 32, "ordinal header at {at}");
            assert_eq!(
                (u16_at(res, at + 8), u16_at(res, at + 12)),
                (0xFFFF, 0xFFFF)
            );
            let (kind, name) = (u16_at(res, at + 10), u16_at(res, at + 14));
            let data = &res[at + header..at + header + size];
            out.push((kind, name, u16_at(res, at + 22), data));
            at = (at + header + size).next_multiple_of(4);
        }
        assert_eq!(at, res.len());
        out
    }

    /// (key, value, children) of a version block, checking its length and alignment.
    fn parse_block(b: &[u8]) -> (String, &[u8], Vec<&[u8]>) {
        let (len, value_len, kind) = (u16_at(b, 0) as usize, u16_at(b, 2), u16_at(b, 4));
        assert_eq!(len, b.len());
        let key: Vec<u16> = (6..len)
            .step_by(2)
            .map(|at| u16_at(b, at))
            .take_while(|&c| c != 0)
            .collect();
        let mut at = (6 + 2 * (key.len() + 1)).next_multiple_of(4);
        let value_bytes = value_len as usize * if kind == 1 { 2 } else { 1 };
        let value = &b[at..at + value_bytes];
        at += value_bytes;
        let mut children = vec![];
        while at < len {
            at = at.next_multiple_of(4);
            let child = u16_at(b, at) as usize;
            children.push(&b[at..at + child]);
            at += child;
        }
        (String::from_utf16(&key).unwrap(), value, children)
    }

    fn text(value: &[u8]) -> String {
        let units: Vec<u16> = value.chunks(2).map(|c| u16_at(c, 0)).collect();
        assert_eq!(units.last(), Some(&0), "NUL-terminated");
        String::from_utf16(&units[..units.len() - 1]).unwrap()
    }

    fn hex(text: &str) -> Vec<u8> {
        let digit = |i| u8::from_str_radix(&text[i..i + 2], 16).unwrap();
        (0..text.len()).step_by(2).map(digit).collect()
    }

    /// Two images: a 2 px 32-bit DIB and the head of a 256 px PNG (entry width and height 0).
    /// llvm-rc reads 40 bytes of each, so both are at least that long.
    const ICO: &str = concat!(
        "0000010002000202000001002000400000002600000000000000010020002800000066000000280000000200",
        "0000040000000100200000000000180000000000000000000000000000000000000000010203040506070809",
        "0a0b0c0d0e0f000000000000000089504e470d0a1a0a0000000d49484452000001000000010008060000005c",
        "72a86600000000000000",
    );

    /// llvm-rc's RT_GROUP_ICON for `1 ICON "x.ico"` with `ICO`.
    const LLVM_RC_GROUP: &str =
        "00000100020002020000010020004000000001000000000001002000280000000200";

    /// llvm-rc's RT_VERSION for the VERSIONINFO statement with `version_info`'s fields at 0.1.1.
    const LLVM_RC_VERSION: &str = concat!(
        "300234000000560053005f00560045005200530049004f004e005f0049004e0046004f0000000000bd04effe",
        "00000100010000000000010001000000000001003f0000000000000004000400010000000000000000000000",
        "0000000090010000010053007400720069006e006700460069006c00650049006e0066006f0000006c010000",
        "01003000340030003900300034004200300000003a0009000100460069006c00650044006500730063007200",
        "69007000740069006f006e00000000007000610069006e0074002e0072007300000000002c00060001004600",
        "69006c006500560065007200730069006f006e000000000030002e0031002e00310000004600110001004c00",
        "6500670061006c0043006f0070007900720069006700680074000000470050004c002d0033002e0030002d00",
        "6f0072002d006c0061007400650072000000000040000c0001004f0072006900670069006e0061006c004600",
        "69006c0065006e0061006d00650000007000610069006e007400720073002e00650078006500000032000900",
        "0100500072006f0064007500630074004e0061006d006500000000007000610069006e0074002e0072007300",
        "00000000300006000100500072006f006400750063007400560065007200730069006f006e00000030002e00",
        "31002e0031000000440000000100560061007200460069006c00650049006e0066006f000000000024000400",
        "00005400720061006e0073006c006100740069006f006e00000000000904b004",
    );

    /// Pinned to the output of llvm-rc 23.1.3 (`/l 0x409`), an independent writer of the format.
    #[test]
    fn res_file_matches_llvm_rc() {
        let ico = hex(ICO);
        let res = res_file(&ico, "0.1.1").unwrap();
        let entries = resources(&res);
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0], (0, 0, 0, &[][..]), "the empty first resource");
        assert_eq!(entries[1], (RT_ICON, 1, EN_US, &ico[38..102]));
        assert_eq!(entries[2], (RT_ICON, 2, EN_US, &ico[102..142]));
        assert_eq!(
            entries[3],
            (RT_GROUP_ICON, 1, EN_US, &hex(LLVM_RC_GROUP)[..])
        );
        assert_eq!(
            entries[4],
            (RT_VERSION, 1, EN_US, &hex(LLVM_RC_VERSION)[..])
        );
        assert_eq!(version_info("0.1.1"), hex(LLVM_RC_VERSION));
    }

    #[test]
    fn res_file_holds_the_icon_group_and_the_version() {
        let ico = hex(ICO);
        let res = res_file(&ico, "1.2.3-rc.1").unwrap();
        let entries = resources(&res);
        assert_eq!(entries[0], (0, 0, 0, &[][..]), "the empty first resource");
        assert_eq!(entries[1], (RT_ICON, 1, EN_US, &ico[38..102]));
        assert_eq!(entries[2], (RT_ICON, 2, EN_US, &ico[102..142]));
        let (kind, name, _, group) = entries[3];
        assert_eq!((kind, name), (RT_GROUP_ICON, 1));
        assert_eq!(group[..6], [0, 0, 1, 0, 2, 0]);
        assert_eq!(group[6..18], ico[6..18], "first entry head");
        assert_eq!(u16_at(group, 18), 1);
        assert_eq!(group[20..32], ico[22..34], "second entry head");
        assert_eq!(u16_at(group, 32), 2);
        assert_eq!(group.len(), 34);

        let (kind, name, _, version) = entries[4];
        assert_eq!((kind, name, entries.len()), (RT_VERSION, 1, 5));
        let (key, fixed, children) = parse_block(version);
        assert_eq!(key, "VS_VERSION_INFO");
        assert_eq!(fixed.len(), 52);
        let fixed: Vec<u32> = (0..13).map(|i| u32_at(fixed, 4 * i)).collect();
        assert_eq!(fixed[0], 0xFEEF04BD);
        assert_eq!(fixed[2..6], [1 << 16 | 2, 3 << 16, 1 << 16 | 2, 3 << 16]);
        assert_eq!(fixed[8..10], [0x0004_0004, 1], "Windows NT application");
        let (string_info, var_info) = (parse_block(children[0]), parse_block(children[1]));
        assert_eq!(string_info.0, "StringFileInfo");
        let table = parse_block(string_info.2[0]);
        assert_eq!(table.0, "040904B0");
        let strings: Vec<(String, String)> = table
            .2
            .iter()
            .map(|s| {
                let (key, value, _) = parse_block(s);
                (key, text(value))
            })
            .collect();
        let get = |k: &str| strings.iter().find(|(key, _)| key == k).unwrap().1.as_str();
        assert_eq!(get("ProductName"), "paint.rs");
        assert_eq!(get("FileDescription"), "paint.rs");
        assert_eq!(get("FileVersion"), "1.2.3-rc.1");
        assert_eq!(get("ProductVersion"), "1.2.3-rc.1");
        assert_eq!(get("LegalCopyright"), "GPL-3.0-or-later");
        assert_eq!(var_info.0, "VarFileInfo");
        let translation = parse_block(var_info.2[0]);
        assert_eq!(translation.0, "Translation");
        assert_eq!(translation.1, [0x09, 0x04, 0xB0, 0x04]);
    }

    #[test]
    fn ico_images_rejects_other_files_and_truncation() {
        assert!(ico_images(b"\x89PNG").is_err());
        let mut ico = [0u16, 1, 1].map(u16::to_le_bytes).concat();
        ico.extend([16, 16, 0, 0, 1, 0, 32, 0, 9, 0, 0, 0, 22, 0, 0, 0]);
        assert!(ico_images(&ico).is_err(), "image bytes missing");
        assert!(ico_images(&ico[..10]).is_err(), "directory cut short");
    }
}
