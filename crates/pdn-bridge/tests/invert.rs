//! End-to-end: start CoreCLR, load the in-tree Invert plugin through the managed
//! C ABI, enumerate it, and render — proving the Rust → hostfxr → PdnBridge → plugin
//! path. Skips (green) when the .NET host/plugin have not been built, so a clean
//! checkout without a .NET SDK still passes. Build them with:
//!   cd pdn-host/bridge   && ./dn.sh build PdnBridge.csproj -c Release -m:1 --disable-build-servers -nodeReuse:false
//!   cd pdn-host/paintnet && python3 tests/run.py selftest
//!
//! Verified on Windows via Wine (the loader's cfg(windows) path): cross-compile the test
//! (`--target x86_64-pc-windows-gnu`, mingw linker), unzip a win-x64 .NET 10 runtime, then
//! run under Wine with Windows-form paths (`winepath -w`): PDN_BRIDGE_DIR → the
//! framework-dependent publish, PDN_SELFTEST_DLL → the plugin, PDN_HOSTFXR + DOTNET_ROOT →
//! the runtime. Do NOT disable mscoree in WINEDLLOVERRIDES — Wine needs it to fix up
//! IL-only assemblies. The render comes back byte-identical to the Linux run.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn repo_rel(p: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(p)
}

#[test]
fn invert_through_abi() {
    let bridge_dir = std::env::var("PDN_BRIDGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_rel("pdn-host/bridge/bin/Release/net11.0"));
    // PDN_SELFTEST_DLL lets the Wine run pass a Windows-form path (winepath -w) that the
    // win-x64 CLR accepts, since the compiled-in repo path is unix-style.
    let plugin = std::env::var("PDN_SELFTEST_DLL")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_rel("pdn-host/paintnet/.build/selftest/PdnSelfTest.dll"));

    if !bridge_dir.join("PdnBridge.dll").exists() || !plugin.exists() {
        eprintln!(
            "skipping: build pdn-host/bridge + the selftest plugin first (see this file's header)"
        );
        return;
    }

    let host = pdn_bridge::Host::start(&bridge_dir).expect("start CoreCLR");
    let plugin = host.load_plugin(&plugin).expect("load plugin");

    let json = plugin.enumerate().expect("enumerate");
    assert!(
        json.contains("PdnSelfTest.InvertEffect"),
        "enumerate: {json}"
    );

    // A small gradient, then render Invert and check R/G/B inverted, alpha kept.
    let (w, h) = (16u32, 16u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        px[0] = (x * 4) as u8;
        px[1] = (y * 4) as u8;
        px[2] = ((x + y) * 2) as u8;
        px[3] = 255;
    }
    let original = rgba.clone();
    let cancel = AtomicBool::new(false);
    plugin
        .render_effect(
            "PdnSelfTest.InvertEffect",
            None,
            &mut rgba,
            w,
            h,
            (0, 0, w as i32, h as i32),
            &cancel,
        )
        .expect("render");

    for (out, src) in rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(original.as_chunks::<4>().0)
    {
        assert_eq!(out[0], 255 - src[0]);
        assert_eq!(out[1], 255 - src[1]);
        assert_eq!(out[2], 255 - src[2]);
        assert_eq!(out[3], src[3]);
    }
}
