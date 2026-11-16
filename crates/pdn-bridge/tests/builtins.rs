//! The classic built-ins (pdn-host's PaintDotNet.Effects.Legacy) through the bridge, driven the way
//! third-party wrappers drive them (the in-tree PdnSelfTest.BuiltinsEffect). Each output is pinned
//! to an FNV-1a hash of pdn-core's genuine-exact port (effect::bb::classic / classic_fx on
//! 080/pdnv5-samples, byte-identical to Paint.NET 5.1.12 on the VM) over the same image; Brightness
//! / Contrast is checked against its formula inline. Also checks that the IndirectUI DisplayName
//! reaches the schema. Skips (green) when the .NET host/plugin are not built (see invert.rs).

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn repo_rel(p: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(p)
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
    })
}

#[test]
fn builtins_match_the_genuine_ports() {
    let bridge_dir = std::env::var("PDN_BRIDGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_rel("pdn-host/bridge/bin/Release/net11.0"));
    let plugin = repo_rel("pdn-host/paintnet/.build/selftest/PdnSelfTest.dll");
    if !bridge_dir.join("PdnBridge.dll").exists() || !plugin.exists() {
        eprintln!("skipping: build pdn-host/bridge + the selftest plugin first (see invert.rs)");
        return;
    }
    let host = pdn_bridge::Host::start(&bridge_dir).expect("start CoreCLR");
    let plugin = host.load_plugin(&plugin).expect("load plugin");
    let id = "PdnSelfTest.BuiltinsEffect";

    let schema = plugin.effect_schema(id).expect("schema");
    assert!(
        schema.contains(r#""label":"Built-in""#),
        "IndirectUI label missing: {schema}"
    );

    let (w, h) = (16u32, 12u32);
    let src: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let a = if (x + y) % 4 == 0 {
                (x * 19 + y * 7) as u8
            } else {
                255
            };
            [
                (x * 37 + y * 11) as u8,
                (x * 5 + y * 29) as u8,
                (x * x + y * 3) as u8,
                a,
            ]
        })
        .collect();
    let render = |which: u32| {
        let mut px = src.clone();
        let props = format!(r#"{{"Which":{which}}}"#);
        plugin
            .render_effect(
                id,
                Some(&props),
                &mut px,
                w,
                h,
                (0, 0, w as i32, h as i32),
                &AtomicBool::new(false),
            )
            .expect("render");
        px
    };

    // Gaussian r3, Median r2 p30, Sharpen 3, Edge Detect 30°, Oil Painting 2/20, Pencil Sketch 2/5.
    let pinned = [
        0x8e35_7438_6cd9_eea4u64,
        0x2d6f_144e_8cf3_062e,
        0x04cd_46cc_03a8_6784,
        0x3a4f_b71f_a1d1_3087,
        0x1d5b_53ed_0a8d_1c92,
        0x65ad_1d4d_a2e2_47eb,
    ];
    for (which, want) in pinned.into_iter().enumerate() {
        assert_eq!(fnv(&render(which as u32)), want, "built-in {which}");
    }

    // Brightness 20, Contrast 40: one intensity-dependent shift for R, G and B; alpha kept.
    let (b, c) = (20, 40);
    let want: Vec<u8> = src
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let i = (19595 * p[0] as i32 + 38470 * p[1] as i32 + 7471 * p[2] as i32) >> 16;
            let shift = (i - 127 + b) * 100 / (100 - c) + 127 - i;
            let ch = |v: u8| (v as i32 + shift).clamp(0, 255) as u8;
            [ch(p[0]), ch(p[1]), ch(p[2]), p[3]]
        })
        .collect();
    assert_eq!(render(6), want, "brightness / contrast");

    // The classic built-ins plug-ins such as pyrochild's Film or toehead2001's Rubber Stamp drive. They
    // re-implement Paint.NET's classic algorithms and need not match genuine pixel for pixel, so Motion
    // Blur 30°/5 and Zoom Blur 30 at (0.25, −0.5) are pinned to this implementation's output.
    assert_eq!(fnv(&render(7)), 0xdb13_e9d8_d7d8_1162, "motion blur");
    assert_eq!(fnv(&render(8)), 0x1169_610c_354a_3b7f, "zoom blur");
    // Pixelate 3: each 3×3 cell from the origin, cut at the image, is the alpha-weighted mean of its
    // four corners.
    let px = |x: u32, y: u32| &src[((y * w + x) * 4) as usize..][..4];
    let want: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let (l, t) = (x - x % 3, y - y % 3);
            let (r, b) = ((l + 2).min(w - 1), (t + 2).min(h - 1));
            let c = [px(l, t), px(r, t), px(l, b), px(r, b)];
            let af: u64 = c.iter().map(|p| p[3] as u64 * 16384).sum();
            let a = ((af + 32768) >> 16) as u8;
            if a == 0 {
                return [0; 4];
            }
            let ch = |k: usize| {
                (c.iter()
                    .map(|p| p[3] as u64 * p[k] as u64 * 16384)
                    .sum::<u64>()
                    / af) as u8
            };
            [ch(0), ch(1), ch(2), a]
        })
        .collect();
    assert_eq!(render(9), want, "pixelate");
    // Add Noise at 0% coverage keeps every pixel; Clouds (Normal, black to white) is opaque grey.
    assert_eq!(render(10), src, "add noise, no coverage");
    let clouds = render(11);
    assert!(
        clouds
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[0] == p[1] && p[1] == p[2] && p[3] == 255),
        "clouds not opaque grey"
    );
}
