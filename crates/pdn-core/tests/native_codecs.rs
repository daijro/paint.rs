//! Task 48: the native AVIF (dav1d over heic-rs's container parser), HEIC (heic-rs, vendored and
//! patched: Ruling R52) and JPEG XR (openreadout-jpegxr: Ruling R51) decoders, and the HEIC/JPEG XR
//! save refusals paintdotjs shows (document-ops-and-formats.md §5 "Task 48"). Hostile inputs for
//! these decoders are in `decode_limits.rs`, under its allocation guard.
//!
//! Fixtures (`tests/fixtures`):
//! - `a.heic`, `a.avif`: `magick -size 13x9 gradient:red-blue a.{heic,avif}` (ImageMagick 7.1 over
//!   libheif 1.23: x265 and aom, 12-bit 4:2:0; the HEIC is a 64 × 64 picture with a 13 × 9 `clap`
//!   and no `colr` box). `heic-alpha.heic`: `heif-enc -q 90 avif-alpha-source.png` (8-bit 4:2:0,
//!   nclx BT.601 full range, alpha). `heic-blocks.heic`: four flat 32 × 32 blocks (orange, teal,
//!   purple, grey), `magick … -quality 90` (8-bit 4:2:0, no `colr` box).
//! - `avif-bt709-limited-alpha.avif` (8-bit 4:2:0, BT.709 limited range, alpha) and
//!   `avif-lossless-alpha.avif` (GBR through the identity matrix, 4:4:4, alpha):
//!   `heif-enc -A --matrix_coefficients=1 --full_range_flag=0 -q 90` and `heif-enc -A -L`
//!   (libheif 1.23 over aom) from `avif-alpha-source.png`, a red-to-blue gradient with a
//!   left-to-right alpha ramp (ImageMagick). `avif-lossless-premultiplied.avif`: that source
//!   premultiplied ((c·a + 127) / 255) and written with `heif-enc -A -L --premultiplied-alpha`
//!   (a `prem` reference). `avif-monochrome.avif`: `heif-enc -A -q 90` from a grey PNG
//!   (`magick -size 13x9 gradient:white-black -colorspace Gray -type Grayscale`), 4:0:0.
//! - `avif-clap-irot.avif` (`avifenc -l --crop 2,1,10,7 --irot 1`) and `avif-imir.avif`
//!   (`avifenc -l --imir 0`) from `avif-alpha-source.png` (libavif 1.4.2). `avif-grid-2x1.avif`:
//!   `avifenc -q 60 --qalpha 60 -g 2x1` from a 128 × 64 red-to-blue gradient with a top-to-bottom
//!   alpha ramp: two 64 × 64 tiles and an alpha grid, 8-bit 4:4:4.
//! - `a.jxr` (24bppBGR): written by a scratch program over jpegxr-pure-rs 0.1.0's lossless encoder.
//!   `a-bgra32.jxr` (32bppBGRA) and `a-rgba64half.jxr` (64bppRGBAHalf) are planar-alpha containers
//!   (T.832 Annex A: ALPHA_OFFSET/ALPHA_BYTE_COUNT) assembled from that encoder's colour-only and
//!   grey codestreams, because its own alpha path writes a zero plane; `a-bgra32.jxr` declares
//!   300 dpi. `a-pbgra32.jxr` (32bppPBGRA) is the same with the colour premultiplied,
//!   (c·a + 127) / 255. All three checked with jxrlib: the npm `jpegxr` 0.3.0 wasm build decodes the colour
//!   exactly (it never asks for planar alpha), and jxrlib's Copy with uAlphaMode 2, as WIC asks,
//!   decodes the alpha planes as written.
//! - `rgb-13x9-q100-24.jxr` and `rgba-13x9-q100-32.jxr`: original 13x9 formula images,
//!   saved and reopened in Paint.NET 5.1.12 on 2026-10-21. Quality 100, explicit 24-bit RGB and
//!   32-bit RGBA. RGB=[20*x,28*y,255-20*x], alpha=255 or 255-25*y; nominal 300 DPI.
//!   Source PNG SHA256: RGB `9b6dc76ee1855c4014a686c8b291be95d26f193cf57673dee67a3399cb7c69c0`,
//!   RGBA `0ecc02dbc527202a127c5fe0af98e3b89fa5e72ae0c937d687e15f137b18d4bd`.
//!   Unchanged native output SHA256: RGB `cc2f79534c9b9cab338ffea82e06904e15c3115970740574e1a6ead1fbae4d56`
//!   (470 bytes), RGBA `77ac0041399ecf0c7c892fba8106b40c2961c6681de5afe8a1799371d0af2a86` (579 bytes).
//!   Capture/probe evidence: `native-observations/2026-10-21-jxr/observations.md` and
//!   `verified-results.json` in the companion observation workspace. All 468 RGBA bytes per image
//!   matched their formula, DPI 299.9989929199219. This covers these tiny lossless encodings only;
//!   it makes no lossy/CMYK/ICC/EXIF/large-image or Windows Rust-build claim.
#![cfg(not(target_arch = "wasm32"))]

use pdn_core::io::codecs::decode;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/").to_owned() + name)
        .unwrap()
}

#[cfg(feature = "avif-dav1d")]
#[test]
fn heic_and_avif_decode_natively() {
    for f in ["heic", "avif"] {
        let bytes = std::fs::read(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/a.").to_owned() + f,
        )
        .unwrap();
        let img = pdn_core::io::codecs::decode(f, &bytes).unwrap().image;
        assert_eq!((img.width, img.height), (13, 9), "{f}");
    }
}

/// Without the `avif-dav1d` feature (no libdav1d on the machine) AVIF decode is refused cleanly.
#[cfg(not(feature = "avif-dav1d"))]
#[test]
fn avif_without_dav1d_is_unsupported() {
    let e = decode("avif", &fixture("a.avif")).unwrap_err();
    assert!(
        matches!(e, pdn_core::io::codecs::CodecError::Unsupported(_)),
        "{e:?}"
    );
}

#[test]
fn heic_and_jxr_saving_is_refused_like_paintdotjs() {
    let img = pdn_core::canvas::ImageData {
        width: 1,
        height: 1,
        data: vec![1, 2, 3, 255],
    };
    for f in ["heic", "jxr"] {
        assert!(
            pdn_core::io::encode::encode(
                &img,
                f,
                &pdn_core::io::encode::SaveOptions::default_for(f)
            )
            .is_err(),
            "{f}"
        );
    }
}

/// `gradient:red-blue` runs from red at the top to blue at the bottom; both fixtures are opaque.
#[test]
fn heic_and_avif_gradients_run_red_to_blue() {
    let formats: &[&str] = if cfg!(feature = "avif-dav1d") {
        &["heic", "avif"]
    } else {
        &["heic"]
    };
    for &f in formats {
        let img = decode(f, &fixture(&format!("a.{f}"))).unwrap().image;
        let px = |x: usize, y: usize| &img.data[(y * 13 + x) * 4..][..4];
        let (top, bottom) = (px(6, 0), px(6, 8));
        assert!(top[0] > 230 && top[2] < 40, "{f} top {top:?}");
        assert!(bottom[2] > 230 && bottom[0] < 40, "{f} bottom {bottom:?}");
        assert!(img.data.chunks(4).all(|p| p[3] == 255), "{f} alpha");
    }
}

/// The AVIF colour paths against libheif 1.23 (ImageMagick's decode of the same files), ±1:
/// 12-bit full-range BT.601 and 8-bit limited-range BT.709 with alpha, both 4:2:0, and 4:0:0.
#[cfg(feature = "avif-dav1d")]
#[test]
fn avif_matches_libheif() {
    type Refs = [((usize, usize), [u8; 4]); 3];
    let cases: [(&str, Refs); 3] = [
        (
            "a.avif",
            [
                ((0, 0), [238, 3, 22, 255]),
                ((6, 4), [112, 4, 150, 255]),
                ((12, 8), [1, 1, 255, 255]),
            ],
        ),
        (
            "avif-bt709-limited-alpha.avif",
            [
                ((0, 0), [239, 3, 16, 76]),
                ((6, 4), [112, 3, 144, 166]),
                ((3, 7), [45, 0, 202, 122]),
            ],
        ),
        (
            "avif-monochrome.avif",
            [
                ((0, 0), [255, 255, 255, 255]),
                ((6, 4), [128, 128, 128, 255]),
                ((12, 8), [0, 0, 0, 255]),
            ],
        ),
    ];
    for (name, refs) in cases {
        let img = decode("avif", &fixture(name)).unwrap().image;
        for ((x, y), want) in refs {
            let got = &img.data[(y * 13 + x) * 4..][..4];
            assert!(
                got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1),
                "{name} ({x}, {y}): {got:?}, libheif {want:?}"
            );
        }
    }
}

/// Lossless AVIF (GBR through the identity matrix, 4:4:4, with alpha) decodes to its source exactly.
#[cfg(feature = "avif-dav1d")]
#[test]
fn avif_lossless_identity_matrix_is_exact() {
    let source = decode("png", &fixture("avif-alpha-source.png"))
        .unwrap()
        .image;
    let avif = decode("avif", &fixture("avif-lossless-alpha.avif"))
        .unwrap()
        .image;
    assert_eq!(avif.data, source.data);
}

/// A `prem` alpha item: the colour is unpremultiplied, back to within rounding of the source
/// (alpha ≥ 77 here, so at most ±2).
#[cfg(feature = "avif-dav1d")]
#[test]
fn avif_premultiplied_alpha_is_unpremultiplied() {
    let source = decode("png", &fixture("avif-alpha-source.png"))
        .unwrap()
        .image;
    let avif = decode("avif", &fixture("avif-lossless-premultiplied.avif"))
        .unwrap()
        .image;
    for (got, want) in avif.data.chunks(4).zip(source.data.chunks(4)) {
        assert_eq!(got[3], want[3]);
        assert!(
            got.iter().zip(want).all(|(g, w)| g.abs_diff(*w) <= 2),
            "{got:?} vs {want:?}"
        );
    }
}

/// Lossless 24bppBGR: pixel (x, y) is RGB [20x, 28y, 255 − 20x], opaque.
#[test]
fn jxr_bgr24_decodes_exactly() {
    let d = decode("jxr", &fixture("a.jxr")).unwrap();
    assert_eq!((d.image.width, d.image.height, d.dpi), (13, 9, None));
    let want: Vec<u8> = (0..9)
        .flat_map(|y| (0..13).flat_map(move |x| [20 * x, 28 * y, 255 - 20 * x, 255]))
        .collect();
    assert_eq!(d.image.data, want);
}

// These are Paint.NET-saved inputs, independent of the synthetic encoder fixtures above.
#[test]
fn jxr_paint_net_quality100_rgb24_is_exact() {
    let decoded = decode("jxr", &fixture("rgb-13x9-q100-24.jxr")).unwrap();
    assert_eq!((decoded.image.width, decoded.image.height), (13, 9));
    let want: Vec<u8> = (0..9)
        .flat_map(|y| (0..13).flat_map(move |x| [20 * x, 28 * y, 255 - 20 * x, 255]))
        .collect();
    assert_eq!(decoded.image.data, want);
    // Native saves report 299.9989929199219, so nominal 300 needs a 0.002 DPI tolerance.
    assert!((decoded.dpi.unwrap() - 300.0).abs() < 0.002);
}

#[test]
fn jxr_paint_net_quality100_rgba32_is_exact() {
    let decoded = decode("jxr", &fixture("rgba-13x9-q100-32.jxr")).unwrap();
    assert_eq!((decoded.image.width, decoded.image.height), (13, 9));
    let want: Vec<u8> = (0..9)
        .flat_map(|y| (0..13).flat_map(move |x| [20 * x, 28 * y, 255 - 20 * x, 255 - 25 * y]))
        .collect();
    assert_eq!(decoded.image.data, want);
    assert!((decoded.dpi.unwrap() - 300.0).abs() < 0.002);
}

/// 64bppRGBAHalf is scRGB (linear): [x/12, y/8, 2 − x/6] encodes to sRGB, clamped; the planar
/// alpha y/8 is straight and linear. Linear 0.5 is sRGB 0.735357 → 188; 0.75 is 0.880828 → 225.
#[test]
fn jxr_half_float_is_linear_scrgb() {
    let img = decode("jxr", &fixture("a-rgba64half.jxr")).unwrap().image;
    let px = |x: usize, y: usize| img.data[(y * 13 + x) * 4..][..4].to_vec();
    assert_eq!(px(0, 0), [0, 0, 255, 0]);
    assert_eq!(px(6, 4), [188, 188, 255, 128]);
    assert_eq!(px(9, 0), [225, 0, 188, 0]);
    assert_eq!(px(12, 8), [255, 255, 0, 255]);
}

/// 32bppBGRA with a planar alpha codestream: RGB [20x, 28y, 255 − 20x], alpha 255 − 25y, exactly,
/// and the container's 300 dpi (WIDTH_RESOLUTION; §2.13: DPI > 0 sets the document's).
#[test]
fn jxr_bgra32_planar_alpha_decodes_exactly() {
    let d = decode("jxr", &fixture("a-bgra32.jxr")).unwrap();
    assert_eq!((d.image.width, d.image.height, d.dpi), (13, 9, Some(300.0)));
    let want: Vec<u8> = (0..9)
        .flat_map(|y| (0..13).flat_map(move |x| [20 * x, 28 * y, 255 - 20 * x, 255 - 25 * y]))
        .collect();
    assert_eq!(d.image.data, want);
}

/// 32bppPBGRA: the colour codestream holds (c·a + 127) / 255, and is unpremultiplied back to the
/// straight source within rounding (alpha ≥ 55 here, so at most ±3); alpha exact.
#[test]
fn jxr_pbgra32_is_unpremultiplied() {
    let img = decode("jxr", &fixture("a-pbgra32.jxr")).unwrap().image;
    for y in 0..9u32 {
        for x in 0..13u32 {
            let got = &img.data[((y * 13 + x) * 4) as usize..][..4];
            let want = [20 * x, 28 * y, 255 - 20 * x].map(|c| c as u8);
            assert_eq!(got[3] as u32, 255 - 25 * y, "alpha ({x}, {y})");
            assert!(
                got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 3),
                "({x}, {y}): {got:?} vs {want:?}"
            );
        }
    }
}

/// An alpha plane interleaved in the image codestream (ALPHA_IMAGE_PLANE_FLAG, the last bit of the
/// header's third flag byte) is not decoded: Unsupported, not a wrong image.
#[test]
fn jxr_interleaved_alpha_is_unsupported() {
    let mut jxr = fixture("a-bgra32.jxr");
    let at = jxr.windows(8).position(|w| w == b"WMPHOTO\0").unwrap();
    jxr[at + 10] |= 1;
    let e = decode("jxr", &jxr).unwrap_err();
    assert!(
        matches!(e, pdn_core::io::codecs::CodecError::Unsupported(_)),
        "{e:?}"
    );
}

/// HEIC against libheif 1.23 (`heif-dec`), ±1. `heic-blocks.heic` (four flat colours) and
/// `a.heic` have no `colr` box, so their colour comes from the HEVC VUI or MIAF's default (BT.601
/// full range), not heic-rs's upstream BT.709 limited fallback (up to 42 off). `a.heic` is checked
/// on its first row only: inside a 4:2:0 gradient heic-rs's sited bilinear chroma and libheif's
/// 12-bit row pairing differ by up to 19 whatever the matrix (the same holds for the 8-bit
/// `heic-alpha.heic`, also a vertical gradient, so its colour is checked on row 0 too).
/// `heic-alpha.heic` has an nclx box and a left-to-right alpha ramp.
#[test]
fn heic_matches_libheif() {
    let cases = [
        (
            "heic-blocks.heic",
            64,
            vec![
                ((16, 16), [230, 121, 30, 255]),
                ((48, 16), [20, 160, 151, 255]),
                ((16, 48), [121, 40, 181, 255]),
                ((48, 48), [128, 128, 128, 255]),
            ],
        ),
        (
            "a.heic",
            13,
            vec![
                ((0, 0), [237, 4, 21, 255]),
                ((6, 0), [237, 4, 21, 255]),
                ((12, 0), [237, 4, 21, 255]),
            ],
        ),
        (
            "heic-alpha.heic",
            13,
            vec![((0, 0), [241, 3, 19, 77]), ((12, 0), [241, 3, 19, 255])],
        ),
    ];
    for (name, w, refs) in cases {
        let img = decode("heic", &fixture(name)).unwrap().image;
        assert_eq!(img.width, w, "{name}");
        for ((x, y), want) in refs {
            let got = &img.data[(y * w as usize + x) * 4..][..4];
            assert!(
                got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1),
                "{name} ({x}, {y}): {got:?}, libheif {want:?}"
            );
        }
    }
    // The alpha plane is luma only, so every pixel can be checked: libheif's ramp, ±1.
    let img = decode("heic", &fixture("heic-alpha.heic")).unwrap().image;
    for ((x, y), want) in [((6, 4), 166), ((12, 8), 255), ((3, 7), 122)] {
        let got = img.data[(y * 13 + x) * 4 + 3];
        assert!(
            got.abs_diff(want) <= 1,
            "alpha ({x}, {y}): {got}, libheif {want}"
        );
    }
}

/// `clap`, `irot` and `imir` are applied in `ipma` order (ISO/IEC 23008-12 §6.5; MIAF requires
/// it), checked against what they mean for the lossless source (libheif 1.23 gives the same):
/// an asymmetric 10 × 7 crop at (2, 1) then a quarter turn anticlockwise (7 × 10), and a
/// top-to-bottom mirror on its own (so a wrong rotation and a wrong mirror cannot cancel out).
#[cfg(feature = "avif-dav1d")]
#[test]
fn avif_transforms_follow_the_spec() {
    let src = decode("png", &fixture("avif-alpha-source.png"))
        .unwrap()
        .image;
    let s = |x: usize, y: usize| src.data[(y * 13 + x) * 4..][..4].to_vec();
    let rotated = decode("avif", &fixture("avif-clap-irot.avif"))
        .unwrap()
        .image;
    assert_eq!((rotated.width, rotated.height), (7, 10));
    let want: Vec<u8> = (0..10)
        .flat_map(|y| (0..7).flat_map(move |x| s(11 - y, 1 + x)))
        .collect();
    assert_eq!(rotated.data, want, "clap + irot");
    let mirrored = decode("avif", &fixture("avif-imir.avif")).unwrap().image;
    let want: Vec<u8> = (0..9)
        .flat_map(|y| (0..13).flat_map(move |x| s(x, 8 - y)))
        .collect();
    assert_eq!(mirrored.data, want, "imir");
}

/// A 2 × 1 grid of 64 × 64 tiles with an alpha grid, against libheif 1.23 (`heif-dec`), ±1; the
/// columns either side of the tile seam pin the tile order.
#[cfg(feature = "avif-dav1d")]
#[test]
fn avif_grid_matches_libheif() {
    let img = decode("avif", &fixture("avif-grid-2x1.avif"))
        .unwrap()
        .image;
    assert_eq!((img.width, img.height), (128, 64));
    for ((x, y), want) in [
        ((0, 0), [1, 0, 254, 254]),
        ((63, 10), [124, 0, 130, 215]),
        ((64, 10), [130, 0, 126, 215]),
        ((100, 40), [201, 0, 54, 93]),
        ((127, 63), [254, 0, 2, 1]),
    ] {
        let got = &img.data[(y * 128 + x) * 4..][..4];
        assert!(
            got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1),
            "({x}, {y}): {got:?}, libheif {want:?}"
        );
    }
}
