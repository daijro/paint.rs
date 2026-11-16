//! Byte-identity: render each available effect through paint.rs's path (pdn-bridge) and
//! through pdn-host's own reflection host (pdnhost), with the same input and settings,
//! and require the output bytes to match. This *measures* the parity the architecture
//! relies on rather than assuming it, across two independent property pipelines (pdnhost's
//! ApplyProps vs PluginHost's CoerceProperty) and a non-strip-multiple size.
//!
//! Covers the in-tree fixtures (Invert, Brightness) always, and the third-party
//! benbuck/MirrorEffect when it has been built under ref/pdn-plugins/mirror/bin (see
//! ref/pdn-plugins/SOURCES.md). Skips (green) when the .NET host/plugins are not built.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

fn repo_rel(p: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(p)
}

fn swap_rb(buf: &mut [u8]) {
    for px in buf.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
}

/// Render through pdnhost (the oracle). `asmdir` holds the replacement PaintDotNet.*.dll.
#[allow(clippy::too_many_arguments)]
fn pdnhost(
    pdnhost_dll: &Path,
    asmdir: &Path,
    plugin_dll: &Path,
    effect: &str,
    props: Option<&str>,
    bgra_in: &[u8],
    w: u32,
    h: u32,
) -> Vec<u8> {
    let tmp = std::env::temp_dir();
    let inp = tmp.join("pdn_parity_in.bgra");
    let outp = tmp.join("pdn_parity_out.bgra");
    std::fs::write(&inp, bgra_in).unwrap();
    let _ = std::fs::remove_file(&outp);
    let mut cmd = Command::new("dotnet");
    cmd.arg("exec")
        .arg(pdnhost_dll)
        .arg("--plugin")
        .arg(plugin_dll)
        .arg("--effect")
        .arg(effect)
        .arg("--asmdir")
        .arg(asmdir)
        .arg("--in")
        .arg(&inp)
        .arg("--out")
        .arg(&outp)
        .arg("--width")
        .arg(w.to_string())
        .arg("--height")
        .arg(h.to_string());
    if let Some(p) = props {
        cmd.arg("--props").arg(p);
    }
    assert!(
        cmd.status().expect("run pdnhost").success(),
        "pdnhost failed for {effect}"
    );
    std::fs::read(&outp).expect("read pdnhost output")
}

#[test]
fn effect_parity_vs_pdnhost() {
    let bridge_dir = std::env::var("PDN_BRIDGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_rel("pdn-host/bridge/bin/Release/net11.0"));
    let selftest_dir = repo_rel("pdn-host/paintnet/.build/selftest");
    let pdnhost_dll = repo_rel("pdn-host/paintnet/.build/host/pdnhost.dll");
    let mirror_dir = repo_rel("ref/pdn-plugins/mirror/bin");

    if !bridge_dir.join("PdnBridge.dll").exists()
        || !selftest_dir.join("PdnSelfTest.dll").exists()
        || !pdnhost_dll.exists()
    {
        eprintln!("skipping: build pdn-host/bridge, the selftest plugin, and pdnhost first");
        return;
    }

    // (plugin dir, dll name, effect id, props) — the plugin dir doubles as the asmdir.
    let mut cases: Vec<(PathBuf, &str, &str, Option<&str>)> = vec![
        (
            selftest_dir.clone(),
            "PdnSelfTest.dll",
            "PdnSelfTest.InvertEffect",
            None,
        ),
        (
            selftest_dir.clone(),
            "PdnSelfTest.dll",
            "PdnSelfTest.BrightnessEffect",
            Some(r#"{"Amount":50}"#),
        ),
        (
            selftest_dir.clone(),
            "PdnSelfTest.dll",
            "PdnSelfTest.BrightnessEffect",
            Some(r#"{"Amount":-40}"#),
        ),
    ];
    if mirror_dir.join("Mirror.dll").exists() {
        cases.push((
            mirror_dir.clone(),
            "Mirror.dll",
            "MirrorEffect.MirrorEffectPlugin",
            None,
        ));
    } else {
        eprintln!("note: ref/pdn-plugins/mirror not built; skipping the third-party Mirror case");
    }

    // A non-64-multiple size exercises PdnBridge's partial last render strip.
    let (w, h) = (50u32, 50u32);
    let mut bgra_in = vec![0u8; (w * h * 4) as usize];
    for (i, px) in bgra_in.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        px[0] = (x * 5) as u8;
        px[1] = (y * 5) as u8;
        px[2] = ((x + y) * 3) as u8;
        px[3] = 255;
    }

    let host = pdn_bridge::Host::start(&bridge_dir).expect("start CoreCLR");

    for (dir, dll, effect, props) in &cases {
        let plugin_dll = dir.join(dll);
        let oracle = pdnhost(
            &pdnhost_dll,
            dir,
            &plugin_dll,
            effect,
            *props,
            &bgra_in,
            w,
            h,
        );

        let plugin = host
            .load_plugin(&plugin_dll)
            .unwrap_or_else(|e| panic!("load {dll}: {e}"));
        let mut rgba = bgra_in.clone();
        swap_rb(&mut rgba);
        let cancel = AtomicBool::new(false);
        plugin
            .render_effect(
                effect,
                *props,
                &mut rgba,
                w,
                h,
                (0, 0, w as i32, h as i32),
                &cancel,
            )
            .unwrap_or_else(|e| panic!("render {effect}: {e}"));
        swap_rb(&mut rgba);

        assert_eq!(rgba.len(), oracle.len(), "{effect}: size differs");
        assert!(
            rgba == oracle,
            "{effect} {props:?}: BYTE MISMATCH vs pdnhost (first diff at {:?})",
            rgba.iter().zip(&oracle).position(|(a, b)| a != b)
        );
        eprintln!(
            "parity OK: {effect} {props:?} ({} bytes identical)",
            oracle.len()
        );
    }
}
