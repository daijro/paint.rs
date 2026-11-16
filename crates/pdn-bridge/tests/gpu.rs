//! End-to-end GPU: load the in-tree SampleGpuEffect plugin through the managed C ABI and exercise each
//! Paint.NET 5 GPU effect base the host supports. The effects use built-in Direct2D effects through the
//! replacement PaintDotNet.Direct2D1 wrappers, which run on opend2d's libd2d1; with an opaque source the
//! float round trip is exact, so the expected images are exact.
//!
//! - ScaleSampleGpuEffect (GpuImageEffect)         -> returns the source: identity
//! - ZoomGpuEffect (PropertyBasedGpuImageEffect)   -> configurable; Zoom=1 identity, Zoom=2 a nearest
//!   upscale through AffineTransform2D (proves the Int32 property reaches the effect through the token)
//! - DrawSampleGpuEffect (GpuDrawingEffect)        -> fills the selection red into its command list
//!
//! Skips (green) when the host, plugin, or native libs are absent. Build: `pdn-host/bridge/publish.sh
//! <dir>` (ships PdnBridge + opend2d's libd2d1/libdwrite), then the plugin:
//!   cd pdn-host/paintnet-gpu && ./dn.sh build plugins/SampleGpuEffect/SampleGpuEffect.csproj \
//!       -c Release --disable-build-servers -nodeReuse:false

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use pdn_bridge::Plugin;

fn repo_rel(p: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(p)
}

/// A gradient with distinct R/G/B so a channel-order slip or a wrong sample shows.
fn gradient(w: u32, h: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        px[0] = (x * 30) as u8;
        px[1] = (y * 30) as u8;
        px[2] = ((x + y) * 15) as u8;
        px[3] = 255;
    }
    rgba
}

fn render(plugin: &Plugin, id: &str, props: Option<&str>, rgba: &mut [u8], w: u32, h: u32) {
    let cancel = AtomicBool::new(false);
    plugin
        .render_effect(id, props, rgba, w, h, (0, 0, w as i32, h as i32), &cancel)
        .unwrap_or_else(|e| panic!("render {id}: {e}"));
}

#[test]
fn gpu_effects_through_abi() {
    let bridge_dir = std::env::var("PDN_BRIDGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_rel("pdn-host/bridge/bin/Release/net11.0"));
    let dll = repo_rel(
        "pdn-host/paintnet-gpu/plugins/SampleGpuEffect/bin/Release/net10.0/SampleGpuEffect.dll",
    );

    let have_bridge_lib = ["libd2d1.so", "libd2d1.dylib", "d2d1.dll"]
        .iter()
        .any(|n| bridge_dir.join(n).exists());
    if !bridge_dir.join("PdnBridge.dll").exists() || !dll.exists() || !have_bridge_lib {
        eprintln!(
            "skipping: build the bridge (publish.sh, ships libd2d1) and SampleGpuEffect first"
        );
        return;
    }

    let host = pdn_bridge::Host::start(&bridge_dir).expect("start CoreCLR");
    let plugin = host.load_plugin(&dll).expect("load plugin");

    // All three list as supported `gpu` effects; the property-based one is configurable, under "Test".
    let json = plugin.enumerate().expect("enumerate");
    for (id, name) in [
        ("SampleGpuEffect.ScaleSampleGpuEffect", "Scale Sample"),
        ("SampleGpuEffect.ZoomGpuEffect", "Zoom Sample"),
        ("SampleGpuEffect.DrawSampleGpuEffect", "Draw Sample"),
    ] {
        assert!(
            json.contains(id) && json.contains(name),
            "enumerate missing {id}/{name}: {json}"
        );
    }
    assert!(json.contains("\"kind\":\"gpu\""), "no gpu kind: {json}");
    assert!(
        json.contains("\"configurable\":true"),
        "property-based effect not configurable: {json}"
    );

    // The Zoom property schema reflects the Int32Property.
    let schema = plugin
        .effect_schema("SampleGpuEffect.ZoomGpuEffect")
        .expect("schema");
    assert!(
        schema.contains("\"Zoom\"")
            && schema.contains("\"type\":\"int\"")
            && schema.contains("\"max\":4"),
        "zoom schema wrong: {schema}"
    );

    let (w, h) = (8u32, 8u32);
    let src = gradient(w, h);

    // 1. Plain GpuImageEffect: identity.
    let mut a = src.clone();
    render(
        &plugin,
        "SampleGpuEffect.ScaleSampleGpuEffect",
        None,
        &mut a,
        w,
        h,
    );
    assert!(
        a == src,
        "ScaleSample identity failed (diff at {:?})",
        a.iter().zip(&src).position(|(p, q)| p != q)
    );

    // 2. GpuDrawingEffect: OnDraw records a red fill of the (whole-image) selection.
    let mut d = src.clone();
    render(
        &plugin,
        "SampleGpuEffect.DrawSampleGpuEffect",
        None,
        &mut d,
        w,
        h,
    );
    assert!(
        d.as_chunks::<4>().0.iter().all(|p| *p == [255, 0, 0, 255]),
        "DrawSample fill wrong: {:?}",
        &d[..16]
    );
    // A selection that is not a rectangle is the selection's geometry: a triangle over the image's top-left
    // half fills red there and leaves the far corner alone.
    let mut t = src.clone();
    let triangle = format!(r#"{{"$environment":{{"selection":[[0,0,{w},0,0,{h}]]}}}}"#);
    render(
        &plugin,
        "SampleGpuEffect.DrawSampleGpuEffect",
        Some(&triangle),
        &mut t,
        w,
        h,
    );
    let tp = t.as_chunks::<4>().0;
    assert_eq!(tp[0], [255, 0, 0, 255], "inside the triangle");
    assert_ne!(tp[tp.len() - 1], [255, 0, 0, 255], "outside the triangle");

    // 3. PropertyBasedGpuImageEffect: Zoom=1 is identity; Zoom=2 is a nearest upscale — a specific
    //    resample that only matches if the Int32 property reached the effect.
    let mut z1 = src.clone();
    render(
        &plugin,
        "SampleGpuEffect.ZoomGpuEffect",
        Some(r#"{"Zoom":1}"#),
        &mut z1,
        w,
        h,
    );
    assert!(
        z1 == src,
        "Zoom=1 identity failed (diff at {:?})",
        z1.iter().zip(&src).position(|(p, q)| p != q)
    );

    let mut z2 = src.clone();
    render(
        &plugin,
        "SampleGpuEffect.ZoomGpuEffect",
        Some(r#"{"Zoom":2}"#),
        &mut z2,
        w,
        h,
    );
    let expected = zoom_resample(&src, w, h, 2);
    assert!(
        z2 != src,
        "Zoom=2 produced identity — the property did not reach the effect"
    );
    assert!(
        z2 == expected,
        "Zoom=2 resample wrong (diff at {:?})",
        z2.iter().zip(&expected).position(|(p, q)| p != q)
    );

    eprintln!("gpu OK: ScaleSample identity, Draw fill, Zoom property resample ({w}x{h})");
}

/// AffineTransform2D nearest at scale `zoom` about the origin: output pixel x samples the source texel
/// under (x + 0.5) / zoom, i.e. x / zoom.
fn zoom_resample(src: &[u8], w: u32, h: u32, zoom: u32) -> Vec<u8> {
    let mut out = vec![0u8; src.len()];
    for y in 0..h {
        for x in 0..w {
            let s = ((y / zoom * w + x / zoom) * 4) as usize;
            let o = ((y * w + x) * 4) as usize;
            out[o..o + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    out
}
