//! Run each Paint.NET built-in GPU effect pixel shader (phase 5A-5/5A-6) through **crates/d2d's
//! effect graph** -- the `PixelShaderEffect` draw transform in `d2d::effects::shader`, which realises
//! Direct2D's default-vertex-shader varyings (`SV_POSITION`, `SCENE_POSITION`, one `TEXCOORD` per
//! simple input) on the CPU pull model -- and compare the result byte-for-byte with the WARP oracle
//! captured by `d2d-ref` (a real `ID2D1Effect` running the same DXBC on the WARP device).
//!
//! This is the route the task asks for in place of the `native/d2dgpu` d2d-shader bridge: the bridge
//! fills only `SV_Position`, so it can run a shader standalone but not realise the D2D varyings an
//! effect graph supplies. Here the shader runs as an actual `ID2D1Effect` through the graph.
//!
//! Inputs: `effects.json` (the capture manifest), the shader DXBC under `--dxbc-dir` (extracted from
//! the gitignored `ref/pdn-wine` by `extract.py`; never committed), and the committed captures under
//! `conformance/oracle/win11-26100/<scene>.bgra`. A shader whose DXBC or capture is absent is SKIPPED
//! (ref material is gitignored, so this runs green in a clean checkout). Exit non-zero on any FAIL.
//!
//! Usage: `pdngpu-verify EFFECTS_JSON DXBC_DIR ORACLE_DIR`

use d2d::bitmap::Bitmap;
use d2d::effects::{Input, PixelShaderEffect};
use d2d::{bitmap_target, Factory};
use std::rc::Rc;
use windows_core::GUID;

/// The 32x32 premultiplied-opaque gradient fed to every single-input shader (same pixels as the
/// committed `fxshader-01` scene): `r=8x, g=8y, b=4(x+y), a=255`, clamped. Opaque so premul == straight.
fn gradient() -> Vec<u8> {
    let mut v = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32u32 {
        for x in 0..32u32 {
            v.extend_from_slice(&[(8 * x).min(255) as u8, (8 * y).min(255) as u8, (4 * (x + y)).min(255) as u8, 255]);
        }
    }
    v
}

/// Register `dxbc` as a custom pixel-shader effect and draw it over the gradient through crates/d2d's
/// effect graph, returning the 32x32 premultiplied BGRA8 target -- the exact path `d2d-ref` drives on
/// WARP for the same effect.
fn render(id: GUID, dxbc: Vec<u8>, cbuffer: Vec<u8>) -> Vec<u8> {
    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_target_bitmap(bitmap_target(32, 32, 96.0));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let xml = "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnGpu'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='pdngpu'/>\
<Inputs><Input name='Source0'/></Inputs></Effect>";
    factory
        .register_effect_from_string(id, xml, Rc::new(move || {
            Box::new(PixelShaderEffect { shader_id: id, dxbc: dxbc.clone(), cbuffer: cbuffer.clone(), input_count: 1, precision: None })
        }))
        .expect("register shader effect");
    let eff = dc.create_effect(id).expect("create effect (shader rejected by executor?)");
    eff.set_input(0, Input::Bitmap(Rc::new(Bitmap::from_straight_rgba(32, 32, &gradient()))));
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    dc.take_target().unwrap().data
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let (json, dxbc_dir, oracle_dir) = (&args[1], &args[2], &args[3]);
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(json).unwrap()).unwrap();
    let (mut pass, mut fail, mut skip) = (0, 0, 0);
    for (idx, c) in manifest["capture"].as_array().unwrap().iter().enumerate() {
        let name = c["name"].as_str().unwrap();
        let scene = c["scene"].as_str().unwrap();
        let dxbc_path = format!("{dxbc_dir}/{name}.dxbc");
        let oracle_path = format!("{oracle_dir}/{scene}.bgra");
        let Ok(dxbc) = std::fs::read(&dxbc_path) else {
            println!("SKIP  {scene}  ({dxbc_path} absent -- run extract.py)");
            skip += 1;
            continue;
        };
        let cbuffer: Vec<u8> = c["cbuffer"].as_array().unwrap().iter().flat_map(|f| (f.as_f64().unwrap() as f32).to_le_bytes()).collect();
        // Unique CLSID per shader: effect registration is keyed by GUID, so a shared one would make
        // every `create_effect` resolve to the first registered shader.
        let id = GUID::from_u128(0x7e3a9f10_2b4c_4d5e_8f60_000000000000 | (idx as u128));
        let got = render(id, dxbc, cbuffer);
        let Ok(want) = std::fs::read(&oracle_path) else {
            // Shader ran through the graph with no unsupported opcode; no oracle yet (VM capture
            // pending). Confirms crates/d2d executes it, but is not counted as a pass.
            println!("RENDERED {scene}  (ran through crates/d2d; no WARP oracle yet; {} bytes)", got.len());
            skip += 1;
            continue;
        };
        if got == want {
            println!("PASS  {scene}  ({} bytes byte-identical to WARP)", got.len());
            pass += 1;
        } else {
            let diff = got.iter().zip(&want).filter(|(a, b)| a != b).count();
            println!("FAIL  {scene}  ({diff}/{} bytes differ)", got.len());
            fail += 1;
        }
    }
    println!("\n{pass} byte-identical, {fail} differ, {skip} skipped (of {} captures)", manifest["capture"].as_array().unwrap().len());
    if fail > 0 {
        std::process::exit(1);
    }
}
