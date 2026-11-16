//! Render a native paint.rs effect to a .bgra file, for byte-comparison against genuine Paint.NET
//! (vm-oracle). Usage: classic_oracle <effect-id> <in.bgra> <out.bgra> <w> <h> [json-overrides]
//! Input/output are straight BGRA8 (pdnhost's format); ImageData is straight RGBA8, so swap R/B.
use pdn_core::canvas::ImageData;
use pdn_core::effect::bb::indirect_defaults;
use pdn_core::effect::schedule::classic_tile_rois;
use pdn_core::effect::{EffectEnv, Values, apply, find};
use pdn_core::pixel::IntRect;

/// Drive a ROI-dependent helper per tile exactly as genuine's ClassicEffectDriver does: for each
/// tile the renderer is called with that tile's ROI rects; the tile's rects are copied into the
/// output. (whole-image per tile — fine for an oracle.)
fn tiled(w: u32, h: u32, render: impl Fn(&[IntRect]) -> ImageData) -> ImageData {
    // Exactly what apply() does: one render with ALL tile rects as env.rois. For a blur (per-pixel
    // independent; rois only pick truncate-vs-round) this equals genuine's per-tile batching.
    let all: Vec<IntRect> = classic_tile_rois(w, h, &[IntRect::new(0, 0, w, h)])
        .into_iter()
        .flatten()
        .collect();
    render(&all)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    // "selscans:<scale>:x1,y1,x2,y2,..." dumps the port's pixel::mask::scans for a polygon, one per-row
    // span "y x0 x1", to compare against genuine PdnRegion scans (pdnhost --selscans).
    if let Some(spec) = a[1].strip_prefix("selscans:") {
        let (sc, coords) = spec.split_once(':').unwrap();
        let scale: f64 = sc.parse().unwrap();
        let nums: Vec<f64> = coords.split(',').map(|s| s.parse().unwrap()).collect();
        let poly: Vec<[f64; 2]> = nums.chunks(2).map(|c| [c[0], c[1]]).collect();
        for (y, x0, x1) in pdn_core::pixel::mask::scans(&[poly], scale) {
            println!("{y} {x0} {x1}");
        }
        return;
    }
    // "blendcheck" dumps pixel::ops::blend over the same fixed (ca, cb, alpha) sweep as pdnhost
    // --blendcheck (genuine ColorBgra.Blend), to byte-check the AA selection-edge composite primitive.
    if a[1] == "blendcheck" {
        // Colors B,G,R,A — must match pdnhost's BlendSweepColors exactly. pixel::ops::blend takes
        // RGBA, so pass each as [R,G,B,A] and print the result back as B G R A.
        let colors: [[u8; 4]; 13] = [
            [0, 0, 0, 0],
            [255, 255, 255, 255],
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [10, 20, 30, 255],
            [10, 20, 30, 128],
            [200, 100, 50, 64],
            [5, 250, 5, 1],
            [128, 128, 128, 127],
            [17, 33, 49, 200],
            [240, 15, 1, 250],
            [1, 1, 1, 2],
        ];
        let alphas: [u8; 13] = [0, 1, 2, 63, 64, 65, 127, 128, 129, 191, 192, 254, 255];
        let to_rgba = |c: [u8; 4]| [c[2], c[1], c[0], c[3]]; // BGRA tuple -> RGBA for ops::blend
        let mut out = String::new();
        for ca in colors {
            for cb in colors {
                for m in alphas {
                    let o = pdn_core::pixel::ops::blend(to_rgba(ca), to_rgba(cb), m);
                    out.push_str(&format!("{} {} {} {}\n", o[2], o[1], o[0], o[3])); // RGBA -> B G R A
                }
            }
        }
        print!("{out}");
        return;
    }
    // "blendopcheck" dumps pixel::ops::compose(mode, d=lhs, s=rhs, 255) over the same (mode, lhs, rhs)
    // sweep as pdnhost --blendop (genuine CompositionOp.Apply), to byte-check every layer UserBlendOp.
    if a[1] == "blendopcheck" {
        use pdn_core::document::layer::BlendMode::*;
        let modes = [
            Normal, Multiply, Additive, ColorBurn, ColorDodge, Reflect, Glow, Overlay, Difference,
            Negation, Lighten, Darken, Screen, Xor,
        ];
        let names = [
            "Normal",
            "Multiply",
            "Additive",
            "ColorBurn",
            "ColorDodge",
            "Reflect",
            "Glow",
            "Overlay",
            "Difference",
            "Negation",
            "Lighten",
            "Darken",
            "Screen",
            "Xor",
        ];
        let colors: [[u8; 4]; 13] = [
            [0, 0, 0, 0],
            [255, 255, 255, 255],
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [10, 20, 30, 255],
            [10, 20, 30, 128],
            [200, 100, 50, 64],
            [5, 250, 5, 1],
            [128, 128, 128, 127],
            [17, 33, 49, 200],
            [240, 15, 1, 250],
            [1, 1, 1, 2],
        ];
        let to_rgba = |c: [u8; 4]| [c[2], c[1], c[0], c[3]];
        let mut out = String::new();
        for (mode, name) in modes.into_iter().zip(names) {
            for ca in colors {
                for cb in colors {
                    let o = pdn_core::pixel::ops::compose(mode, to_rgba(ca), to_rgba(cb), 255);
                    out.push_str(&format!("{name} {} {} {} {}\n", o[2], o[1], o[0], o[3]));
                }
            }
        }
        print!("{out}");
        return;
    }
    let (id, inp, outp) = (&a[1], &a[2], &a[3]);
    let (w, h): (u32, u32) = (a[4].parse().unwrap(), a[5].parse().unwrap());
    let overrides: serde_json::Value = a
        .get(6)
        .map(|s| serde_json::from_str(s).unwrap())
        .unwrap_or(serde_json::json!({}));

    let mut rgba = std::fs::read(inp).unwrap();
    for p in rgba.as_chunks_mut::<4>().0 {
        p.swap(0, 2); // BGRA -> RGBA
    }
    let src = ImageData {
        width: w,
        height: h,
        data: rgba,
    };
    // "selblur:<r>:x1,y1,x2,y2,..." runs the full selection path the real app does: aliased selection
    // mask -> bi-level coverage scans -> square-tile schedule -> gaussian_blur_legacy over those ROIs
    // -> underwrite (hard clip for an aliased/pixelated selection). Oracle vs genuine GaussianBlurEffect
    // under `pdnhost --tiles --selpoly`. Proves the blur truncates at the selection scan-lefts, not x=0.
    if let Some(spec) = id.strip_prefix("selblur:") {
        use pdn_core::effect::bb::classic::gaussian_blur_legacy;
        use pdn_core::pixel::mask::{SelectionQuality, rasterize};
        let (r, coords) = spec.split_once(':').unwrap();
        let r: u32 = r.parse().unwrap();
        let nums: Vec<f64> = coords.split(',').map(|s| s.parse().unwrap()).collect();
        let poly: Vec<Vec<[f64; 2]>> = vec![nums.chunks(2).map(|c| [c[0], c[1]]).collect()];
        let whole = IntRect::new(0, 0, w, h);
        let mask = rasterize(&poly, SelectionQuality::Aliased, whole);
        let region = mask.coverage_scans();
        let tiles: Vec<IntRect> = classic_tile_rois(w, h, &region)
            .into_iter()
            .flatten()
            .collect();
        let mut out = gaussian_blur_legacy(&src, r, &tiles);
        pdn_core::effect::underwrite(&src, &mut out, &mask);
        let mut bgra = out.data;
        for p in bgra.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
        std::fs::write(outp, &bgra).unwrap();
        eprintln!("rendered {id} -> {outp} ({w}x{h})");
        return;
    }
    // "fn:<name>:<a>:<b>" renders a classic helper directly (to oracle it vs a genuine PDN effect).
    let out = if let Some(spec) = id.strip_prefix("fn:") {
        use pdn_core::effect::bb::classic::{
            gaussian_blur, gaussian_blur_legacy, oil_painting, pencil_sketch, percentile_filter,
        };
        let p: Vec<&str> = spec.split(':').collect();
        let n = |i: usize| p[i].parse::<i64>().unwrap();
        // ROI-independent helpers render once; ROI-dependent (blur / pencil) drive per tile.
        match p[0] {
            "median" => percentile_filter(&src, n(1) as u32, n(2) as f32 / 100.0),
            "oilpaint" => oil_painting(&src, n(1) as u32, n(2) as u32),
            "pencil" => tiled(w, h, |rois| {
                pencil_sketch(&src, n(1) as u32, n(2) as i32, rois)
            }),
            "blur" => gaussian_blur(&src, n(1) as u32),
            "bluleg" => tiled(w, h, |rois| gaussian_blur_legacy(&src, n(1) as u32, rois)),
            other => panic!("unknown fn:{other}"),
        }
    } else {
        let def = find(id).unwrap_or_else(|| panic!("no effect {id}"));
        let env = EffectEnv::whole(w, h);
        let mut v: Values = indirect_defaults(&def, &env);
        if let serde_json::Value::Object(o) = overrides {
            v.extend(o);
        }
        // PDNGPU_F32=<path>: for a GPU effect, dump the pre-quantization f32 the quantizer would round
        // (RGBA LE), so the sweep can prove a port↔genuine ±1 is a tie (frac(v·255)≈0.5), not a bug.
        if let Ok(fpath) = std::env::var("PDNGPU_F32")
            && let pdn_core::effect::EffectKind::Gpu(info, render) = def.kind
        {
            let srf =
                pdn_core::effect::surface::EffectSurface::from_rgba8(&src, info.space, info.input);
            let mut genv = env;
            genv.render_info = Some(info);
            let f = render(&srf, &v, &genv).prequant_f32(info.space, info.output);
            std::fs::write(
                &fpath,
                f.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>(),
            )
            .unwrap();
            eprintln!("dumped prequant f32 ({} floats) -> {fpath}", f.len());
            return;
        }
        apply(&src, &def, &v, &env)
    };
    let mut bgra = out.data;
    for p in bgra.as_chunks_mut::<4>().0 {
        p.swap(0, 2); // RGBA -> BGRA
    }
    std::fs::write(outp, &bgra).unwrap();
    eprintln!("rendered {id} -> {outp} ({w}x{h})");
}
