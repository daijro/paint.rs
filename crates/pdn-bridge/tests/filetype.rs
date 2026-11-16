//! FileType plugin path end-to-end: load the bundled WebP FileType plugin (with the
//! native Linux codec `.so` beside it), save a BGRA image to WebP through the plugin,
//! and open it back — a lossless round-trip must be pixel-exact. Proves the ABI's
//! FileType save/open and the native-codec redirection.
//!
//! Skips (green) unless the test plugin dir exists (it is never committed — the plugin
//! DLL is Paint.NET's). Stage it with:
//!   cd pdn-host/paintnet-filetypes
//!   sh native/webp/build.sh native/webp/libWebP_x64.so
//!   mkdir -p .build/webp-plugin
//!   cp <Bundled>/WebPFileType/WebPFileType.dll native/webp/libWebP_x64.so .build/webp-plugin/

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

fn repo_rel(p: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(p)
}

#[test]
fn webp_filetype_roundtrip() {
    let bridge_dir = std::env::var("PDN_BRIDGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_rel("pdn-host/bridge/bin/Release/net11.0"));
    let webp_dir = std::env::var("PDN_WEBP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_rel("pdn-host/paintnet-filetypes/.build/webp-plugin"));
    let dll = webp_dir.join("WebPFileType.dll");

    if !bridge_dir.join("PdnBridge.dll").exists() || !dll.exists() {
        eprintln!("skipping: stage the WebP test plugin dir first (see this file's header)");
        return;
    }

    let host = pdn_bridge::Host::start(&bridge_dir).expect("start CoreCLR");
    let plugin = host.load_plugin(&dll).expect("load WebP plugin");

    let json = plugin.enumerate().expect("enumerate");
    assert!(
        json.contains(".webp"),
        "enumerate had no .webp FileType: {json}"
    );
    // The save options (paint.rs's Save dialog builds its controls from them), labelled.
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let props = parsed["fileTypes"][0]["properties"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let names: Vec<&str> = props.iter().filter_map(|p| p["name"].as_str()).collect();
    assert!(
        names.contains(&"Quality") && names.contains(&"Lossless"),
        "WebP's save properties are missing: {json}"
    );

    let (w, h) = (16u32, 16u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        px[0] = (x * 16) as u8;
        px[1] = (y * 16) as u8;
        px[2] = ((x + y) * 8) as u8;
        px[3] = 255;
    }

    let bytes = plugin
        .filetype_save(0, Some(r#"{"Lossless":true}"#), &rgba, w, h)
        .expect("save WebP");
    assert!(
        bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        "not a WebP container"
    );

    let (back, bw, bh) = plugin.filetype_open(0, &bytes, None).expect("open WebP");
    assert_eq!((bw, bh), (w, h), "dimensions changed");
    assert_eq!(back, rgba, "lossless WebP round-trip was not pixel-exact");
    eprintln!(
        "WebP FileType round-trip OK: {} bytes, {bw}x{bh} pixel-exact",
        bytes.len()
    );
}

/// A classic (Paint.NET 3.x to 5.1) FileType: the free Paint Shop Pro FileType by null54
/// (github.com/0xC0000054/pdn-pspformat, v1.0.3; under the gitignored ref/pdn-plugins, see its
/// SOURCES.md). A PropertyBasedFileType: its save properties are listed, and an image saved with a
/// non-default one opens back pixel-exact (PSP is lossless). Skips unless $PDN_PSP_DIR holds it.
#[test]
fn classic_filetype_roundtrip() {
    let bridge_dir = std::env::var("PDN_BRIDGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_rel("pdn-host/bridge/bin/Release/net11.0"));
    let dll = std::env::var("PDN_PSP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_rel("ref/pdn-plugins/pspformat/bin"))
        .join("PaintShopProFiletype.dll");
    if !bridge_dir.join("PdnBridge.dll").exists() || !dll.exists() {
        eprintln!(
            "skipping: needs the bridge and ref/pdn-plugins/pspformat/bin (see this test's doc)"
        );
        return;
    }
    let host = pdn_bridge::Host::start(&bridge_dir).expect("start CoreCLR");
    let plugin = host.load_plugin(&dll).expect("load the PSP FileType");
    let json = plugin.enumerate().expect("enumerate");
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let ft = &parsed["fileTypes"][0];
    assert!(json.contains(".pspimage"), "no .pspimage FileType: {json}");
    let props = ft["properties"].as_array().cloned().unwrap_or_default();
    assert!(!props.is_empty(), "no save properties: {json}");
    // The last choice of the first choice property (a compression or version), to prove it applies.
    let choice = props
        .iter()
        .find(|p| p["type"] == "choice")
        .expect("a choice property");
    let last = choice["choices"].as_array().unwrap().len() - 1;
    let options = serde_json::json!({ choice["name"].as_str().unwrap(): last }).to_string();

    let (w, h) = (13u32, 9u32);
    let rgba: Vec<u8> = (0..w * h)
        .flat_map(|i| [(i * 7) as u8, (i * 3) as u8, (255 - i) as u8, 255])
        .collect();
    for opts in [None, Some(options.as_str())] {
        let bytes = plugin.filetype_save(0, opts, &rgba, w, h).expect("save");
        let (back, bw, bh) = plugin.filetype_open(0, &bytes, None).expect("open");
        assert_eq!((bw, bh), (w, h));
        assert_eq!(back, rgba, "the PSP round trip changed pixels ({opts:?})");
    }
}
