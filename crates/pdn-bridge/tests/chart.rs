//! Microsoft's chart control (System.Windows.Forms.DataVisualization) through the bridge, driven the way
//! BoltBait's Chart or Graph drives it: the in-tree PdnSelfTest.ChartEffect, a classic effect with its own
//! config token, loads a chart from the token's XML and draws it with Chart.SaveImage over the headless
//! System.Windows.Forms stand-in and GDI+. Rendered with no dialog, it gets its token's defaults (three
//! red columns). Skips (green) when the bridge (publish.sh, which ships libgdiplus off Windows) or the
//! selftest plugin is not built (see invert.rs).

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn repo_rel(p: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(p)
}

#[test]
fn chart_renders_through_abi() {
    let bridge_dir = std::env::var("PDN_BRIDGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_rel("pdn-host/bridge/bin/Release/net11.0"));
    let plugin = repo_rel("pdn-host/paintnet/.build/selftest/PdnSelfTest.dll");
    let gdiplus = cfg!(windows)
        || ["libgdiplus.so", "libgdiplus.dylib"]
            .iter()
            .any(|n| bridge_dir.join(n).exists());
    if !bridge_dir.join("PdnBridge.dll").exists() || !plugin.exists() || !gdiplus {
        eprintln!("skipping: publish pdn-host/bridge (GDI+) and build the selftest plugin first");
        return;
    }
    let host = pdn_bridge::Host::start(&bridge_dir).expect("start CoreCLR");
    let plugin = host.load_plugin(&plugin).expect("load plugin");

    let (w, h) = (96u32, 72u32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    plugin
        .render_effect(
            "PdnSelfTest.ChartEffect",
            None,
            &mut px,
            w,
            h,
            (0, 0, w as i32, h as i32),
            &AtomicBool::new(false),
        )
        .expect("render");
    let pixels = px.as_chunks::<4>().0;
    // The chart's white background, and its columns filled pure red.
    assert_eq!(pixels[0], [255, 255, 255, 255], "background");
    let red = pixels.iter().filter(|p| **p == [255, 0, 0, 255]).count();
    assert!(red > 100, "{red} red column pixels");
}
