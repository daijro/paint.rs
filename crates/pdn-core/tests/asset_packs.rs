//! AssetPackManager.js admission, spec §0.6 and R101/R103.
use pdn_core::asset_pack::{MAX_IMAGE_RGBA, MAX_PACK_RGBA, read_pack};
use std::io::{Cursor, Write};
const MANIFEST: &[u8] = br#"{"formatVersion":1,"name":"Test","id":"t"}"#;
const PNG: &[u8] = include_bytes!("../../../tests/fixtures/icon-64.png");
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(vec![]));
    for (name, bytes) in entries {
        w.start_file(
            *name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap().into_inner()
}
#[test]
fn reads_valid_pack() {
    let p = read_pack(&zip(&[
        ("pack/asset-pack.json", MANIFEST),
        ("pack/icons/menu_file_new_icon.png", PNG),
        ("pack/lang/de.json", br#"{"a":"b"}"#),
    ]))
    .unwrap();
    assert_eq!(p.id, "t");
    assert!(p.files.contains_key("assets/icons/menu_file_new_icon.png"));
    assert_eq!(p.translations["de"], r#"{"a":"b"}"#);
}
#[test]
fn rejects_unsafe_and_ambiguous_archives() {
    for path in [
        "../evil.png",
        "/absolute.png",
        "C:/evil.png",
        "./C:/evil.png",
        ".//Z:relative.png",
        "other/../evil",
        "other\\..\\evil",
    ] {
        assert!(
            read_pack(&zip(&[
                ("pack/asset-pack.json", MANIFEST),
                ("pack/icons/a.png", PNG),
                (path, b"x")
            ]))
            .is_err(),
            "{path}"
        );
    }
    for extra in [
        "asset-pack.json",
        "second/asset-pack.json",
        "pack/./icons/a.png",
        "pack/icons//a.png",
    ] {
        let payload = if extra.ends_with("json") {
            MANIFEST
        } else {
            PNG
        };
        assert!(
            read_pack(&zip(&[
                ("pack/asset-pack.json", MANIFEST),
                ("pack/icons/a.png", PNG),
                (extra, payload)
            ]))
            .is_err(),
            "{extra}"
        );
    }
    let p = read_pack(&zip(&[
        ("asset-pack.json", MANIFEST),
        ("icons/A.png", PNG),
        ("icons/a.png", PNG),
    ]))
    .unwrap();
    assert_eq!(p.files.len(), 2);
}
#[test]
fn enforces_manifest_and_content_contract() {
    for m in [
        r#"{"formatVersion":2,"name":"x"}"#.to_string(),
        r#"{"formatVersion":1,"name":" "}"#.into(),
        format!(r#"{{"formatVersion":1,"name":"{}"}}"#, "😀".repeat(41)),
        r#"{"formatVersion":1,"name":"x","id":2}"#.into(),
    ] {
        assert!(
            read_pack(&zip(&[
                ("asset-pack.json", m.as_bytes()),
                ("icons/a.png", PNG)
            ]))
            .is_err()
        );
    }
    assert!(read_pack(&zip(&[("asset-pack.json", MANIFEST)])).is_err());
    assert!(
        read_pack(&zip(&[
            ("asset-pack.json", MANIFEST),
            ("lang/en.json", b"[]")
        ]))
        .is_err()
    );
    assert!(
        read_pack(&zip(&[
            ("asset-pack.json", MANIFEST),
            ("icons/bad.png", b"bad")
        ]))
        .is_err()
    );
}
#[test]
fn counts_ignored_entries_and_decompressed_bytes() {
    let names = (0..1000)
        .map(|i| format!("ignored/{i}"))
        .collect::<Vec<_>>();
    let mut entries = vec![("asset-pack.json", MANIFEST), ("icons/a.png", PNG)];
    entries.extend(names.iter().map(|n| (n.as_str(), &b"x"[..])));
    assert!(read_pack(&zip(&entries)).is_err());
    let bomb = vec![0; 61 * 1024 * 1024];
    assert!(
        read_pack(&zip(&[
            ("pack/asset-pack.json", MANIFEST),
            ("pack/icons/a.png", PNG),
            ("ignored/bomb", &bomb)
        ]))
        .is_err()
    );
    assert!(read_pack(&vec![0; 25 * 1024 * 1024 + 1]).is_err());
}
#[test]
fn limits_final_rgba_even_for_grayscale_pngs() {
    assert_eq!(MAX_IMAGE_RGBA, 64 * 1024 * 1024);
    assert_eq!(MAX_PACK_RGBA, 256 * 1024 * 1024);
    let image = image::GrayImage::new(4097, 4097);
    let mut png = Cursor::new(vec![]);
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    assert!(
        read_pack(&zip(&[
            ("asset-pack.json", MANIFEST),
            ("icons/a.png", png.get_ref())
        ]))
        .is_err()
    );
}
fn force_zip64(bytes: &[u8]) -> Vec<u8> {
    let at = bytes.len() - 22;
    let count = u16::from_le_bytes(bytes[at + 10..at + 12].try_into().unwrap()) as u64;
    let size = u32::from_le_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as u64;
    let offset = u32::from_le_bytes(bytes[at + 16..at + 20].try_into().unwrap()) as u64;
    let mut out = bytes[..at].to_vec();
    out.extend(b"PK\x06\x06");
    out.extend(44u64.to_le_bytes());
    out.extend(45u16.to_le_bytes());
    out.extend(45u16.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend(size.to_le_bytes());
    out.extend(offset.to_le_bytes());
    out.extend(b"PK\x06\x07");
    out.extend(0u32.to_le_bytes());
    out.extend((at as u64).to_le_bytes());
    out.extend(1u32.to_le_bytes());
    let mut end = bytes[at..].to_vec();
    end[8..12].fill(255);
    end[12..20].fill(255);
    out.extend(end);
    out
}
#[test]
fn admits_small_forced_zip64_and_rejects_hostile_footer_candidates() {
    let ordinary = zip(&[("asset-pack.json", MANIFEST), ("icons/a.png", PNG)]);
    let good = force_zip64(&ordinary);
    assert!(read_pack(&good).is_ok());
    let footer = ordinary.len() - 22;
    for (at, data) in [
        (footer + 32, u64::MAX.to_le_bytes()),
        (footer + 40, u64::MAX.to_le_bytes()),
        (footer + 56 + 8, u64::MAX.to_le_bytes()),
    ] {
        let mut bad = good.clone();
        bad[at..at + 8].copy_from_slice(&data);
        assert!(read_pack(&bad).is_err());
    }
    for end in [good.len() - 1, footer + 8, footer + 60] {
        assert!(read_pack(&good[..end]).is_err());
    }
    // A small last EOCD may fail directory parsing, causing zip2 to retry an unsafe earlier one.
    let mut fallback = ordinary.clone();
    fallback[footer + 8..footer + 12].copy_from_slice(&1001u16.to_le_bytes().repeat(2));
    let mut last = ordinary[footer..].to_vec();
    last[8..12].copy_from_slice(&3u16.to_le_bytes().repeat(2));
    fallback.extend(last);
    assert!(read_pack(&fallback).is_err());
}
#[test]
fn accepts_utf8_bom_and_numeric_version_one() {
    let manifest = b"\xef\xbb\xbf{\"formatVersion\":1.0,\"name\":\"Test\"}";
    let p = read_pack(&zip(&[
        ("asset-pack.json", manifest),
        ("lang/en.json", b"\xef\xbb\xbf{\"k\":\"value\"}"),
    ]))
    .unwrap();
    let dictionary: serde_json::Value = serde_json::from_str(&p.translations["en"]).unwrap();
    assert_eq!(dictionary["k"], "value");
}
#[test]
fn rejects_actual_expansion_with_falsified_declared_size() {
    let bomb = vec![0; 61 * 1024 * 1024];
    let mut bytes = zip(&[
        ("asset-pack.json", MANIFEST),
        ("icons/a.png", PNG),
        ("ignored.bin", &bomb),
    ]);
    let at = bytes
        .windows(4)
        .enumerate()
        .filter(|(_, v)| *v == b"PK\x01\x02")
        .map(|(i, _)| i)
        .next_back()
        .unwrap();
    bytes[at + 24..at + 28].copy_from_slice(&1u32.to_le_bytes());
    assert!(read_pack(&bytes).is_err());
}
#[test]
fn rejects_aggregate_decoded_rgba_even_when_each_image_fits() {
    let image = image::GrayImage::new(4096, 4096);
    let mut png = Cursor::new(vec![]);
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let paths = (0..5).map(|i| format!("icons/{i}.png")).collect::<Vec<_>>();
    let mut files = vec![("asset-pack.json", MANIFEST)];
    files.extend(paths.iter().map(|s| (s.as_str(), png.get_ref().as_slice())));
    assert!(
        read_pack(&zip(&files))
            .unwrap_err()
            .to_string()
            .contains("aggregate")
    );
    // Once four images consume the budget, reject a fifth from dimensions before its pixels.
    let mut broken = png.get_ref().clone();
    let idat = broken.windows(4).position(|v| v == b"IDAT").unwrap();
    broken[idat + 5] ^= 0xff;
    files.pop();
    files.push((paths[4].as_str(), &broken));
    assert!(
        read_pack(&zip(&files))
            .unwrap_err()
            .to_string()
            .contains("aggregate")
    );
}
#[test]
fn rejects_duplicate_raw_names_hidden_by_zip_reader() {
    let mut bytes = zip(&[
        ("asset-pack.json", MANIFEST),
        ("icons/a.png", PNG),
        ("icons/b.png", PNG),
    ]);
    let positions = bytes
        .windows(b"icons/b.png".len())
        .enumerate()
        .filter(|(_, w)| *w == b"icons/b.png")
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    for at in positions {
        bytes[at + 6] = b'a';
    }
    assert!(read_pack(&bytes).is_err());
}
