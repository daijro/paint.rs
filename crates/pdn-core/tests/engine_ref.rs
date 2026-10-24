//! Byte-exact comparisons with paintdotjs engine code: tests/engine-ref holds its output, frozen.
use pdn_core::canvas::ImageData;
use pdn_core::canvas::blur::{gaussian_box_radii, paintdotjs_cpu_box_blur_premul};
use pdn_core::testing::{Tolerance, assert_engine_ref, engine_ref_json, fixture_rgba, js_num};
use pdn_core::util::{
    color::Color,
    geom::{Point, Rectangle},
    shortcut::ShortcutKey,
    utility,
};

#[test]
fn fixture_matches_js() {
    assert_engine_ref(
        "fixture-8x4-seed3",
        &fixture_rgba(8, 4, 3),
        Tolerance::Exact,
    );
}

/// BitmapEffectEngine CPU Gaussian: gaussianBoxRadii + 3 boxBlur passes, byte-exact.
#[test]
fn gaussian_box_blur_matches_js() {
    for case in engine_ref_json("blur-gaussian-cpu").as_array().unwrap() {
        let sigma = case[0].as_f64().unwrap();
        let radii: Vec<f64> = case[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_f64().unwrap())
            .collect();
        assert_eq!(gaussian_box_radii(sigma, 3), radii, "sigma {sigma}");
    }
    let mut out = vec![];
    for sigma in [3.0, 1.3] {
        let mut data = fixture_rgba(24, 16, 3);
        paintdotjs_cpu_box_blur_premul(&mut data, 24, 16, sigma);
        out.extend(data);
    }
    assert_engine_ref("blur-gaussian-cpu", &out, Tolerance::Exact);
}

#[test]
fn color_conversions_match_js() {
    let expected = engine_ref_json("color-conversions");
    let mut out = vec![];
    for r in (0..256).step_by(51) {
        for g in (0..256).step_by(51) {
            for b in (0..256).step_by(51) {
                let c = Color::new(r as u8, g as u8, b as u8, 200);
                out.push(serde_json::json!([
                    js_num(c.get_hue()),
                    js_num(c.get_saturation()),
                    js_num(c.get_lightness()),
                    js_num(c.get_hsv_saturation()),
                    js_num(c.get_value()),
                    c.to_hex(),
                    c.to_packed()
                ]));
                let h = Color::from_hsl(c.get_hue(), c.get_saturation(), c.get_lightness(), 200);
                let v = Color::from_hsv(c.get_hue(), c.get_hsv_saturation(), c.get_value(), 200);
                out.push(serde_json::json!([
                    h.red, h.green, h.blue, v.red, v.green, v.blue
                ]));
            }
        }
    }
    for (i, (a, b)) in out.iter().zip(expected.as_array().unwrap()).enumerate() {
        assert_eq!(a, b, "entry {i}");
    }
    assert_eq!(out.len(), expected.as_array().unwrap().len());
}

fn nums(v: &[f64]) -> serde_json::Value {
    serde_json::Value::Array(v.iter().map(|&x| js_num(x)).collect())
}

#[test]
fn utility_geometry_matches_js() {
    let e = engine_ref_json("utility-geometry");
    let p = Point::new;
    let poly = [
        p(-5.0, 3.0),
        p(20.0, -4.0),
        p(37.0, 18.0),
        p(12.0, 40.0),
        p(-8.0, 22.0),
    ];
    let clipped: Vec<_> = utility::sutherland_hodgman(&Rectangle::new(0.0, 0.0, 32.0, 32.0), &poly)
        .iter()
        .map(|q| nums(&[q.x, q.y]))
        .collect();
    assert_eq!(serde_json::Value::Array(clipped), e["clipped"]);
    let scans: Vec<_> = utility::get_scans(&[p(2.0, 1.0), p(14.0, 3.0), p(9.0, 12.0), p(1.0, 9.0)])
        .iter()
        .map(|s| serde_json::json!([s.x, s.y, s.length]))
        .collect();
    assert_eq!(serde_json::Value::Array(scans), e["scans"]);
    let r = utility::points_to_rectangle(p(5.0, 9.0), p(2.0, 3.0));
    assert_eq!(nums(&[r.x, r.y, r.width, r.height]), e["rect"]);
    let r2 = Rectangle::intersect_of(
        &Rectangle::new(0.0, 0.0, 10.0, 10.0),
        &Rectangle::new(5.0, 5.0, 10.0, 10.0),
    );
    assert_eq!(nums(&[r2.x, r2.y, r2.width, r2.height]), e["r2"]);
}

#[test]
fn shortcut_keys_match_js() {
    let e = engine_ref_json("shortcut-keys");
    let combos = [
        "Ctrl+Shift+S",
        "Ctrl++",
        "Alt+PageUp",
        "Delete",
        "ctrl+alt+shift+v",
        "F4",
        "Ctrl+,",
        "Esc",
        "Cmd+Z",
    ];
    let out: Vec<_> = combos
        .iter()
        .map(|c| {
            let k = ShortcutKey::from_combo(c);
            serde_json::json!([k.to_string(), k.key, k.shift, k.ctrl, k.alt, k.meta])
        })
        .collect();
    assert_eq!(serde_json::Value::Array(out), e);
}

#[test]
fn settings_defaults_match_js() {
    // paintdotjs's defaults with Paint.NET's (R19) overrides.
    let mut js = engine_ref_json("settings-defaults");
    pdn_core::settings::apply_r19_defaults(&mut js);
    assert_eq!(pdn_core::settings::AppSettings::defaults(), js);
}

#[test]
fn i18n_matches_js() {
    let en = include_str!("../../../assets/lang/en.json");
    let i = pdn_core::i18n::I18n::new(en);
    let got = serde_json::json!([
        i.t("aboutDialog.text.format", &["paint.js"]),
        i.t("menu.file.saveAs.text", &[]),
        i.t("no.such.key", &[]),
        i.t("addNewBlankLayer.layerName.format", &["3"]),
        i.t("toolsControl.toolToolTip.format", &["Lasso Select", "S, S"]),
    ]);
    assert_eq!(got, engine_ref_json("i18n-samples"));
}

fn pdn_fixture() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/engine-ref/pdn-encode-3-layers.bin"
    ))
    .unwrap()
}

#[test]
fn pdn_decodes_paintdotjs_output_with_paint_net_precedence() {
    let doc = pdn_core::io::pdn::decode(&pdn_fixture()).unwrap();
    assert_eq!((doc.width, doc.height, doc.layers.len()), (19, 11, 3));
    assert_eq!(doc.layers[1].0.name, "Ünïcode layer");
    assert_eq!(doc.layers[1].0.opacity, 190);
    // document-ops-and-formats.md §2.11 Read step 5: the NormalBlendOp class paintdotjs writes
    // wins over blendMode = Screen.
    assert_eq!(doc.layers[1].0.blend_mode.value(), "normal");
    assert!(!doc.layers[2].0.visible);
    assert_eq!(doc.layers[1].1.data, {
        // The fixture layer, straight RGBA (the page round-trips it through a canvas, so only
        // fully opaque pixels are compared exactly).
        let src = fixture_rgba(19, 11, 7);
        let got = &doc.layers[1].1.data;
        src.chunks(4)
            .zip(got.chunks(4))
            .flat_map(|(s, g)| if s[3] == 255 { s.to_vec() } else { g.to_vec() })
            .collect::<Vec<u8>>()
    });
}

/// Read step 4: chunks may arrive in any order, each exactly once; a short chunk leaves zeros.
#[test]
fn deferred_blocks_take_chunks_in_any_order() {
    use pdn_core::io::pdn::{gzip_member, read_deferred_block};
    let block = |chunks: &[(u32, &[u8])]| {
        let mut b = vec![0u8];
        b.extend(4u32.to_be_bytes());
        for (i, data) in chunks {
            let m = gzip_member(data);
            b.extend(i.to_be_bytes());
            b.extend((m.len() as u32).to_be_bytes());
            b.extend(m);
        }
        b
    };
    let reversed = block(&[(1, &[5, 6, 7, 8]), (0, &[1, 2, 3, 4])]);
    assert_eq!(
        read_deferred_block(&reversed, 8).unwrap(),
        (vec![1, 2, 3, 4, 5, 6, 7, 8], reversed.len())
    );
    let short = block(&[(0, &[1, 2, 3, 4]), (1, &[5])]);
    assert_eq!(
        read_deferred_block(&short, 8).unwrap().0,
        [1, 2, 3, 4, 5, 0, 0, 0]
    );
    assert!(read_deferred_block(&block(&[(0, &[1, 2, 3, 4]), (0, &[1, 2, 3, 4])]), 8).is_err());
    assert!(read_deferred_block(&block(&[(0, &[1, 2, 3, 4]), (2, &[1, 2, 3, 4])]), 8).is_err());
}

#[test]
fn pdn_corrupt_inputs() {
    // Review Focus 5.
    let js = pdn_fixture();
    for cut in [4, 10, js.len() / 3, js.len() - 5] {
        assert!(pdn_core::io::pdn::decode(&js[..cut]).is_err(), "cut {cut}");
    }
    let mut bad = js.clone();
    bad[4] = 0xFF;
    bad[5] = 0xFF;
    bad[6] = 0xFF; // absurd XML length
    assert!(pdn_core::io::pdn::decode(&bad).is_err());
    let mut v4 = js.clone();
    v4[3] = b'4'; // "unsupported version"
    assert!(pdn_core::io::pdn::decode(&v4).is_err());
    // Every truncation and every single-byte corruption returns (Ok or Err), never panics.
    for cut in 0..js.len() {
        let _ = pdn_core::io::pdn::decode(&js[..cut]);
    }
    for at in (0..js.len()).step_by(3) {
        let mut b = js.clone();
        b[at] ^= 0xA5;
        let _ = pdn_core::io::pdn::decode(&b);
    }
}

/// Ruling R41: real Paint.NET 5.1.12 writes its version into the graph's `Document.savedWith`
/// (BB: icon-lemon.pdn holds 5.112.9563.32325) while the templates paintdotjs patches hold
/// 0.0.0.0. Writes the four `System.Version` Int32s (`_Major` … `_Revision`) into `graph`; the
/// values follow the record's metadata, which ends with "_Revision" and four Int32 type bytes.
fn with_pdn_saved_with(mut graph: Vec<u8>) -> Vec<u8> {
    let meta = b"\x09_Revision\x00\x00\x00\x00\x08\x08\x08\x08";
    let at = graph.windows(meta.len()).position(|w| w == meta).unwrap() + meta.len();
    assert_eq!(
        graph[at..at + 16],
        [0; 16],
        "the template's savedWith is 0.0.0.0"
    );
    for (k, v) in [5i32, 112, 9563, 32325].into_iter().enumerate() {
        graph[at + 4 * k..at + 4 * k + 4].copy_from_slice(&v.to_le_bytes());
    }
    graph
}

/// §5 Task 49: graph bytes equal paintdotjs's template output for an all-Normal document, apart
/// from savedWith (`with_pdn_saved_with`).
#[test]
fn pdn_graph_matches_templates_for_normal_layers() {
    let js = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/engine-ref/pdn-encode-3-normal.bin"
    ))
    .unwrap();
    let doc = pdn_core::io::pdn::decode(&js).unwrap();
    let ours = pdn_core::io::pdn::encode(&doc.to_document()).unwrap();
    let graph = |b: &[u8]| {
        let xml_len = b[4] as usize | (b[5] as usize) << 8 | (b[6] as usize) << 16;
        b[7 + xml_len + 2..pdn_core::io::pdn::graph_end(b).unwrap()].to_vec()
    };
    assert_eq!(graph(&ours), with_pdn_saved_with(graph(&js)));
    let back = pdn_core::io::pdn::decode(&ours).unwrap();
    for (a, b) in doc.layers.iter().zip(back.layers.iter()) {
        assert_eq!((&a.0, &a.1.data), (&b.0, &b.1.data));
    }
}

/// Each layer's blendOp class follows its mode; re-decoding keeps it (Read step 5).
#[test]
fn pdn_writes_blend_op_classes() {
    let js = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/engine-ref/pdn-encode-3-normal.bin"
    ))
    .unwrap();
    let mut d = pdn_core::io::pdn::decode(&js).unwrap().to_document();
    d.layers.get_at_mut(1).unwrap().properties.blend_mode =
        pdn_core::document::layer::BlendMode::Screen;
    let bytes = pdn_core::io::pdn::encode(&d).unwrap();
    let name = b"PaintDotNet.UserBlendOps+ScreenBlendOp";
    assert!(bytes.windows(name.len()).any(|w| w == name));
    let back = pdn_core::io::pdn::decode(&bytes).unwrap();
    assert_eq!(back.layers[1].0.blend_mode.value(), "screen");
    // LayerBlendMode.value__ follows the mode too (Screen = 12), not only the blendOp class.
    let xml_len = bytes[4] as usize | (bytes[5] as usize) << 8 | (bytes[6] as usize) << 16;
    let (root, _) = pdn_core::io::nrbf::deserialize(&bytes[7 + xml_len + 2..]).unwrap();
    let layer = &root
        .get("layers")
        .get("ArrayList+_items")
        .as_array()
        .unwrap()[1];
    assert_eq!(
        layer
            .get("Layer+properties")
            .get("blendMode")
            .get("value__")
            .as_i64(),
        Some(12)
    );
}

/// Review Focus 2: the colour of α = 0 pixels and low alpha survive a .pdn round trip.
#[test]
fn pdn_round_trip_keeps_hidden_colour() {
    let mut doc = pdn_core::document::Document::new(2, 1, 96.0);
    let mut l = pdn_core::document::layer::Layer::create_layer(2, 1, "a");
    l.surface.set_pixel(0, 0, [10, 20, 30, 0]);
    l.surface.set_pixel(1, 0, [200, 100, 50, 1]);
    doc.add_layer(l);
    let back = pdn_core::io::pdn::decode(&pdn_core::io::pdn::encode(&doc).unwrap()).unwrap();
    assert_eq!(back.layers[0].1.data, [10, 20, 30, 0, 200, 100, 50, 1]);
}

/// §2.11 Write: gzip member header, savedWithVersion, thumbnail box, no layer cap.
#[test]
fn pdn_container_details() {
    use pdn_core::io::pdn::{GZIP_MEMBER_HEADER, encode, thumb_size};
    assert_eq!(
        GZIP_MEMBER_HEADER,
        [0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0x00, 0x0a]
    );
    assert_eq!(
        (
            thumb_size(1511, 1503),
            thumb_size(300, 200),
            thumb_size(100, 50)
        ),
        ((256, 254), (256, 170), (100, 50))
    );
    // §2.11 thumbnail note (analyst resolution): the max-edge rule, the short side truncated.
    assert_eq!(thumb_size(300, 301), (255, 256));
    assert_eq!(thumb_size(256, 100), (256, 100));
    let mut doc = pdn_core::document::Document::new(1, 1, 96.0);
    for i in 0..65 {
        doc.add_layer(pdn_core::document::layer::Layer::create_layer(
            1,
            1,
            &format!("L{i}"),
        ));
    }
    let bytes = encode(&doc).unwrap(); // 65 layers: the 64-layer template cap is gone
    let xml = String::from_utf8_lossy(&bytes[7..200]);
    assert!(
        xml.contains(r#"layers="65" savedWithVersion="5.112.9563.32325""#),
        "{xml}"
    );
    assert!(xml.contains("<thumb png="));
    assert!(bytes.windows(10).any(|w| w == GZIP_MEMBER_HEADER));
    assert_eq!(pdn_core::io::pdn::decode(&bytes).unwrap().layers.len(), 65);
}

/// Task 49b: encode → decode gives the document back for every blend mode, opacity, visibility,
/// name and size (a layer over 256 KiB spans two pixel chunks; over 256 px the thumbnail scales).
/// An empty name stays empty (only a missing name becomes "Layer N").
#[test]
fn pdn_round_trips_every_layer_property() {
    use pdn_core::document::{
        Document,
        layer::{BlendMode, Layer},
    };
    use pdn_core::io::pdn::{GZIP_MEMBER_HEADER, decode, encode, graph_end};
    for (w, h, dpi) in [(1, 1, 96.0), (19, 11, 300.0), (300, 250, 72.5)] {
        let mut doc = Document::new(w, h, dpi);
        for (i, mode) in BlendMode::ALL.into_iter().enumerate() {
            let name = ["Ünïcode 层 ✓", "", "plain"][i % 3];
            let mut l = Layer::create_layer(w, h, name);
            l.surface
                .data_mut()
                .copy_from_slice(&fixture_rgba(w, h, i as u32));
            l.properties.blend_mode = mode;
            l.properties.opacity = (i * 37 % 256) as u8;
            l.properties.visible = i % 4 != 1;
            l.properties.is_background = i == 0;
            doc.add_layer(l);
        }
        let bytes = encode(&doc).unwrap();
        let back = decode(&bytes).unwrap();
        assert_eq!(
            (back.width, back.height, back.resolution, back.layers.len()),
            (w, h, dpi, 14)
        );
        for (i, (l, (p, img))) in doc.layers.iter().zip(&back.layers).enumerate() {
            assert_eq!(p, &l.properties, "{w}x{h} layer {i}");
            assert_eq!(img.data, l.surface.data(), "{w}x{h} layer {i}");
        }
        // Every pixel chunk is its own 5.1.12 gzip member: one per 256 KiB per layer.
        let pixels = &bytes[graph_end(&bytes).unwrap()..];
        let members = pixels
            .windows(10)
            .filter(|m| *m == GZIP_MEMBER_HEADER)
            .count();
        assert_eq!(members, 14 * (w * h * 4).div_ceil(262144) as usize);
    }
}

/// Task 49 follow-up: the reader's value budget grows with the input, so a legitimate file with
/// 12 000 layers (past the fixed 256k-value budget) reopens.
#[test]
fn pdn_reopens_twelve_thousand_layers() {
    use pdn_core::document::{Document, layer::Layer};
    let mut doc = Document::new(1, 1, 96.0);
    for i in 0..12_000 {
        doc.add_layer(Layer::create_layer(1, 1, &format!("L{i}")));
    }
    let bytes = pdn_core::io::pdn::encode(&doc).unwrap();
    let back = pdn_core::io::pdn::decode(&bytes).unwrap();
    assert_eq!(back.layers.len(), 12_000);
    assert_eq!(back.layers[11_999].0.name, "L11999");
}

/// §2.11 step 5 and §6 item 11 (inferred until B-F7): the first instance of a class carries
/// its metadata and later compatible ones are ClassWithId; a BitmapLayerProperties whose blendOp
/// type differs from the first one's gets full metadata again (BinaryFormatter keeps the first
/// metadata per class name).
#[test]
fn pdn_blend_op_metadata_follows_binary_formatter() {
    use pdn_core::document::{
        Document,
        layer::{BlendMode, Layer},
    };
    let mut doc = Document::new(1, 1, 96.0);
    for mode in [
        BlendMode::Normal,
        BlendMode::Screen,
        BlendMode::Screen,
        BlendMode::Normal,
    ] {
        let mut l = Layer::create_layer(1, 1, "x");
        l.properties.blend_mode = mode;
        doc.add_layer(l);
    }
    let bytes = pdn_core::io::pdn::encode(&doc).unwrap();
    let count = |s: &str| {
        bytes
            .windows(s.len())
            .filter(|w| *w == s.as_bytes())
            .count()
    };
    // The BitmapLayer metadata names the class once; layers 0, 1 and 2 carry full metadata.
    assert_eq!(
        count("PaintDotNet.BitmapLayer+BitmapLayerProperties"),
        1 + 3
    );
    // Layers 1 and 2's metadata name ScreenBlendOp, and so does the first ScreenBlendOp record.
    assert_eq!(count("PaintDotNet.UserBlendOps+ScreenBlendOp"), 2 + 1);
    // Layer 0's metadata and the first NormalBlendOp record; layer 3 reuses layer 0's metadata.
    assert_eq!(count("PaintDotNet.UserBlendOps+NormalBlendOp"), 1 + 1);
    let modes: Vec<_> = pdn_core::io::pdn::decode(&bytes)
        .unwrap()
        .layers
        .iter()
        .map(|l| l.0.blend_mode)
        .collect();
    assert_eq!(
        modes,
        [
            BlendMode::Normal,
            BlendMode::Screen,
            BlendMode::Screen,
            BlendMode::Normal
        ]
    );
}

/// DDS is a bundled Paint.NET plugin (not analyzable, spec §0.2): paintdotjs's encoder is the behaviour (R11).
#[test]
fn dds_encoder_matches_paintdotjs() {
    let e = engine_ref_json("encoders-dds");
    let img = ImageData {
        width: 13,
        height: 9,
        data: fixture_rgba(13, 9, 5),
    };
    // The JS read pixels back through getImageData: apply the same premultiply round trip.
    let mut cv = pdn_core::canvas::Canvas::new(13, 9);
    cv.put_image_data(&img, 0, 0);
    let img = cv.get_image_data(0, 0, 13, 9);
    let bytes = pdn_core::io::encode::encode(&img, "dds", &Default::default()).unwrap();
    assert_eq!(serde_json::json!(bytes), e["dds"]);
}

#[test]
fn jpeg_flattens_onto_white_and_uses_4_4_0() {
    use pdn_core::io::encode::{SaveOptions, encode};
    let img = ImageData {
        width: 16,
        height: 16,
        data: [0u8, 0, 0, 0].repeat(256),
    };
    let bytes = encode(&img, "jpeg", &SaveOptions::default_for("jpeg")).unwrap();
    let back = pdn_core::io::codecs::decode("jpeg", &bytes).unwrap().image;
    assert!(
        back.data.chunks(4).all(|p| p[0] >= 253 && p[3] == 255),
        "transparent → white (document-ops-and-formats.md §2.8 step 6)"
    );
    // SOF0 component sampling: Y = H1 V2 (0x12), Cb = Cr = 0x11 for 4:4:0. Walk the marker segments.
    let mut at = 2;
    while bytes[at + 1] != 0xC0 {
        at += 2 + u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]) as usize;
    }
    assert_eq!(
        [bytes[at + 11], bytes[at + 14], bytes[at + 17]],
        [0x12, 0x11, 0x11]
    );
}

#[test]
fn roundtrip_plugin_formats() {
    let img = ImageData {
        width: 13,
        height: 9,
        data: fixture_rgba(13, 9, 4),
    };
    for f in ["webp", "dds"] {
        let bytes = pdn_core::io::encode::encode(&img, f, &Default::default()).unwrap();
        assert_eq!(
            pdn_core::io::codecs::decode(f, &bytes).unwrap().image.data,
            img.data,
            "{f}"
        );
    }
    let avif = cfg!(feature = "avif-dav1d").then_some("avif");
    for f in ["jpeg", "jxl"].into_iter().chain(avif) {
        let mut o = pdn_core::io::encode::SaveOptions::default_for(f);
        o.quality = Some(95);
        let bytes = pdn_core::io::encode::encode(&img, f, &o).unwrap();
        let back = pdn_core::io::codecs::decode(f, &bytes).unwrap().image;
        assert_eq!((back.width, back.height), (13, 9), "{f}");
        // Lossy at q95, but close: the mean channel error stays small (measured 5.1 for JPEG, 3.0 for
        // JPEG XL, 1.3 for AVIF through dav1d).
        let error: u32 = back
            .data
            .iter()
            .zip(&img.data)
            .map(|(a, b)| a.abs_diff(*b) as u32)
            .sum();
        assert!(
            error < 8 * img.data.len() as u32,
            "{f}: mean error {}",
            error as f64 / img.data.len() as f64
        );
    }
}

#[test]
fn detection_by_extension_then_content() {
    use pdn_core::io::codecs::detect_format;
    assert_eq!(
        detect_format("a.JPG", None, b"not really a jpeg"),
        Some("jpeg")
    );
    assert_eq!(
        detect_format("noext", None, &[0x89, b'P', b'N', b'G', 13, 10, 26, 10]),
        Some("png")
    );
    assert_eq!(detect_format("x.bin", None, b"PDN3abc"), Some("pdn"));
    assert_eq!(detect_format("x.bin", Some("image/gif"), &[]), Some("gif"));
    assert_eq!(detect_format("x.ico", None, &[0, 0, 1, 0]), None); // ICO is not supported (R20)
}

#[test]
fn hostile_inputs_error_not_panic() {
    // Review Focus 5.
    let jpeg = pdn_core::io::encode::encode(
        &ImageData {
            width: 4,
            height: 4,
            data: fixture_rgba(4, 4, 2),
        },
        "jpeg",
        &Default::default(),
    )
    .unwrap();
    for f in [
        "png", "jpeg", "gif", "bmp", "tiff", "tga", "webp", "dds", "jxl", "avif",
    ] {
        assert!(
            pdn_core::io::codecs::decode(f, &jpeg[..jpeg.len() / 2]).is_err() || f == "jpeg",
            "{f} truncated"
        );
        assert!(pdn_core::io::codecs::decode(f, &[]).is_err(), "{f} empty");
    }
    // A TGA header claiming 40000 × 40000 is rejected by the 32768 px guard before allocating.
    let mut huge = vec![0u8, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    huge.extend(40_000u16.to_le_bytes());
    huge.extend(40_000u16.to_le_bytes());
    huge.extend([32, 0]);
    assert!(pdn_core::io::codecs::decode("tga", &huge).is_err());
}

use pdn_core::io::encode::{
    BitDepth, Depth, SaveOptions, allowed_depths, choose_bit_depth, encode,
};

fn solid(w: u32, h: u32, colours: &[[u8; 4]]) -> ImageData {
    ImageData {
        width: w,
        height: h,
        data: (0..w * h)
            .flat_map(|i| colours[i as usize % colours.len()])
            .collect(),
    }
}

/// document-ops-and-formats.md §5 Task 47: TGA 2×1 opaque red, Auto, RLE → 543 bytes.
#[test]
fn tga_writer_is_byte_exact() {
    let clock = pdn_core::io::tga::TgaClock {
        year: 2026,
        month: 10,
        day: 18,
        hour: 12,
        minute: 0,
        second: 0,
    };
    let bytes = pdn_core::io::tga::encode(
        &solid(2, 1, &[[255, 0, 0, 255]]),
        &SaveOptions::default_for("tga"),
        clock,
    );
    assert_eq!(
        &bytes[..18],
        [0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 0, 24, 0]
    );
    assert_eq!(&bytes[18..22], [0x81, 0x00, 0x00, 0xFF]);
    assert_eq!(bytes.len(), 543);
    assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 495);
    let footer = &bytes[517..];
    assert_eq!(u32::from_le_bytes(footer[0..4].try_into().unwrap()), 22);
    assert_eq!(&footer[8..], b"TRUEVISION-XFILE.\0");
    assert_eq!(
        &bytes[22 + 2 + 41 + 324 + 12 + 41 + 6..][..16],
        b"Paint.NET 5.1.12"
    );
}

/// §5: RLE rows [A,A,A,B,C,C] → 82 A | 00 B | 81 C; [A,B,B] → 00 A | 81 B (packets never cross rows).
#[test]
fn tga_rle_packets() {
    let (a, b, c) = ([1u8, 2, 3, 255], [4u8, 5, 6, 255], [7u8, 8, 9, 255]);
    let clock = pdn_core::io::tga::TgaClock {
        year: 2026,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
    };
    let row = |px: &[[u8; 4]]| {
        let bytes = pdn_core::io::tga::encode(
            &ImageData {
                width: px.len() as u32,
                height: 1,
                data: px.concat(),
            },
            &SaveOptions::default_for("tga"),
            clock,
        );
        bytes[18..bytes.len() - 495 - 26].to_vec()
    };
    let bgr = |p: [u8; 4]| [p[2], p[1], p[0]];
    assert_eq!(
        row(&[a, a, a, b, c, c]),
        [&[0x82][..], &bgr(a), &[0x00], &bgr(b), &[0x81], &bgr(c)].concat()
    );
    assert_eq!(
        row(&[a, b, b]),
        [&[0x00][..], &bgr(a), &[0x81], &bgr(b)].concat()
    );
}

/// §2.8 step 5 and §5 "Auto selection" (no size trials when N > 65536).
#[test]
fn auto_bit_depth() {
    let mut never = |_: Depth| -> usize { unreachable!() };
    let two = solid(300, 300, &[[0, 0, 0, 255], [255, 255, 255, 255]]);
    assert_eq!(
        choose_bit_depth("png", &two, &SaveOptions::default_for("png"), &mut never),
        Depth::B1
    );
    assert_eq!(
        choose_bit_depth("bmp", &two, &SaveOptions::default_for("bmp"), &mut never),
        Depth::B1
    );
    assert_eq!(
        choose_bit_depth("tga", &two, &SaveOptions::default_for("tga"), &mut never),
        Depth::B24
    );
    let holes = solid(300, 300, &[[9, 9, 9, 255], [0, 0, 0, 0]]);
    assert_eq!(
        choose_bit_depth("png", &holes, &SaveOptions::default_for("png"), &mut never),
        Depth::B8A1
    );
    assert_eq!(
        choose_bit_depth("bmp", &holes, &SaveOptions::default_for("bmp"), &mut never),
        Depth::B32
    );
    let soft = solid(300, 300, &[[9, 9, 9, 128]]);
    for f in ["png", "bmp", "tiff", "tga"] {
        assert_eq!(
            choose_bit_depth(f, &soft, &SaveOptions::default_for(f), &mut never),
            Depth::B32,
            "{f}"
        );
    }
    assert_eq!(
        allowed_depths("gif", BitDepth::Auto),
        [Depth::B8, Depth::B8A1]
    );
    // N ≤ 65536, opaque, ≤ 16 colours: the smaller of the 4-bit and 24-bit trial encodes wins (ties → 24).
    let small = solid(64, 64, &[[1, 2, 3, 255], [4, 5, 6, 255], [7, 8, 9, 255]]);
    let mut tie = |_: Depth| 100;
    assert_eq!(
        choose_bit_depth("png", &small, &SaveOptions::default_for("png"), &mut tie),
        Depth::B24
    );
    let mut prefer4 = |d: Depth| if d == Depth::B4 { 90 } else { 100 };
    assert_eq!(
        choose_bit_depth(
            "png",
            &small,
            &SaveOptions::default_for("png"),
            &mut prefer4
        ),
        Depth::B4
    );
}

/// Lossless palette GIF: §5 example (palette order, transparent last, indices).
#[test]
fn gif_lossless_palette() {
    let row = ImageData {
        width: 5,
        height: 1,
        data: [
            [255u8, 0, 0, 255],
            [0, 0, 255, 255],
            [255, 0, 0, 255],
            [7, 7, 7, 0],
            [0, 255, 0, 255],
        ]
        .concat(),
    };
    let (palette, idx) =
        pdn_core::io::encode::indexed(&row, Depth::B8A1, &SaveOptions::default_for("gif"));
    assert_eq!(
        palette,
        [
            [255, 0, 0, 255],
            [0, 0, 255, 255],
            [0, 255, 0, 255],
            [0, 0, 0, 0]
        ]
    );
    assert_eq!(idx, [0, 1, 0, 3, 2]);
    let bytes = encode(&row, "gif", &SaveOptions::default_for("gif")).unwrap();
    let back = pdn_core::io::codecs::decode("gif", &bytes).unwrap().image;
    assert_eq!(
        back.data,
        [
            [255u8, 0, 0, 255],
            [0, 0, 255, 255],
            [255, 0, 0, 255],
            [0, 0, 0, 0],
            [0, 255, 0, 255]
        ]
        .concat()
    );
}

/// §2.8 step 9: PNG chunk order of a metadata-free WIC PNG; pHYs = trunc(dpi / 0.0254).
#[test]
fn png_chunks() {
    let img = ImageData {
        width: 4,
        height: 4,
        data: fixture_rgba(4, 4, 2),
    };
    let bytes = encode(
        &img,
        "png",
        &SaveOptions {
            bit_depth: BitDepth::B24,
            ..SaveOptions::default_for("png")
        },
    )
    .unwrap();
    let mut chunks = vec![];
    let mut at = 8;
    while at < bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        chunks.push(
            std::str::from_utf8(&bytes[at + 4..at + 8])
                .unwrap()
                .to_string(),
        );
        if &bytes[at + 4..at + 8] == b"pHYs" {
            assert_eq!(
                u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()),
                3779
            );
        }
        at += 12 + len;
    }
    assert_eq!(
        chunks
            .iter()
            .filter(|c| c.as_str() != "IDAT")
            .cloned()
            .collect::<Vec<_>>(),
        ["IHDR", "sRGB", "gAMA", "pHYs", "IEND"]
    );
}

#[test]
fn roundtrip_every_writable_format() {
    // Opaque and fully transparent pixels survive every lossless depth; 32-bit keeps α = 0 colour (Review Focus 2).
    let img = ImageData {
        width: 13,
        height: 9,
        data: fixture_rgba(13, 9, 4),
    };
    for f in ["png", "bmp", "tga", "tiff"] {
        let bytes = encode(&img, f, &SaveOptions::default_for(f)).unwrap();
        assert_eq!(
            pdn_core::io::codecs::decode(f, &bytes).unwrap().image.data,
            img.data,
            "{f}"
        );
    }
    let soft = ImageData {
        width: 13,
        height: 9,
        data: fixture_rgba(13, 9, 5),
    };
    let bytes = encode(&soft, "png", &SaveOptions::default_for("png")).unwrap();
    assert_eq!(
        pdn_core::io::codecs::decode("png", &bytes)
            .unwrap()
            .image
            .data,
        soft.data
    );
}
