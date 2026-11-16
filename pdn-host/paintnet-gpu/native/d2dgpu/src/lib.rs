//! The Paint.NET GPU host's native bridge (phase 5A-5).
//!
//! The managed wrapper surface (`hosts/paintnet-gpu/compat`) P/Invokes [`d2dgpu_render`] to run a
//! GPU plugin's precompiled ComputeSharp.D2D1 pixel shader (DXBC, embedded in the plugin) over its
//! input image. The shader is executed by [`d2d_shader`], whose SM4/5 CPU executor matches WARP's
//! JIT bit-for-bit (phase 3C.1), and the input is sampled through crates/d2d's WARP-exact
//! `sampling.rs`. So the bytes the C# layer gets back are the bytes native Direct2D on WARP would
//! produce for the same shader — the owner's parity requirement — on every platform the host runs on.
//!
//! This is the pixel-shader slice of the rendering C ABI; `d2d-ffi` on main exposes only pure-math
//! `d2d1.dll` entry points off Windows, and the `d2d` effect graph does not yet run DXBC (see the
//! README blockers). A shader whose texcoord comes from D2D's scene→texcoord vertex realization
//! (`custom-effects` R11) rather than `SV_Position` needs that realization wired in first; this
//! bridge fills `SV_Position` (the fullscreen-triangle convention `shader-ref` and the `shader-*`
//! oracles use), which is the boundary noted in phase 3C.1.

use d2d_shader::exec::{self, Env, PixelInputs};
use d2d_shader::Shader;

/// `D2D1_EXTEND_MODE` fold of a texel index onto `0..n` — the same rule as crates/d2d's sampler.
#[derive(Clone, Copy)]
enum Extend {
    Clamp,
    Wrap,
    Mirror,
}
impl Extend {
    fn fold(self, c: i32, n: i32) -> i32 {
        match self {
            Extend::Clamp => c.clamp(0, n - 1),
            Extend::Wrap => c.rem_euclid(n),
            Extend::Mirror => {
                let p = 2 * n;
                let m = c.rem_euclid(p);
                if m < n { m } else { p - 1 - m }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    Nearest,
    Linear,
}

/// Status codes returned by [`d2dgpu_render`]. Non-zero is a failure the host surfaces, never a
/// silently wrong image (owner rule: an unsupported opcode is reported, not zeroed).
pub const OK: i32 = 0;
pub const ERR_BAD_DXBC: i32 = -1;
pub const ERR_OUT_SIZE: i32 = -2;
pub const ERR_INPUT_SIZE: i32 = -3;
/// An opcode the executor does not implement: the raw SM4/5 opcode number is returned as `-1000 - op`.
pub const ERR_UNSUPPORTED_BASE: i32 = -1000;

/// One input image: tightly packed RGBA8, top-down. `width == 0` means the effect has no input.
struct Img<'a> {
    data: &'a [u8],
    w: u32,
    h: u32,
    filter: Filter,
    extend: Extend,
}

/// The `Env` the shader samples through. Nearest uses crates/d2d's WARP-exact point sample; linear
/// interpolates the raw UNORM8 bytes and divides by 255 once (byte-domain) — the filter WARP applies
/// when a `sample` feeds a float target, which is *not* the quantised UNORM8 DrawBitmap path.
struct ImgEnv<'a>(Option<Img<'a>>);

impl Img<'_> {
    /// Raw UNORM8 texel (0..=255) at a (possibly out-of-range) index, folded by the extend mode.
    fn texel(&self, x: i32, y: i32) -> [f32; 4] {
        let x = self.extend.fold(x, self.w as i32) as usize;
        let y = self.extend.fold(y, self.h as i32) as usize;
        let i = (y * self.w as usize + x) * 4;
        core::array::from_fn(|k| self.data[i + k] as f32)
    }
}

impl Env for ImgEnv<'_> {
    fn sample(&self, _r: u32, _s: u32, coord: [f32; 4], _mode: d2d_shader::exec::Sampling) -> [f32; 4] {
        let Some(img) = &self.0 else { return [0.0; 4] };
        let (tx, ty) = (coord[0] * img.w as f32, coord[1] * img.h as f32);
        match img.filter {
            // WARP point sample: texel at floor(coord), bytes normalised by /255.
            Filter::Nearest => img.texel(tx.floor() as i32, ty.floor() as i32).map(|c| c / 255.0),
            // Byte-domain bilinear: interpolate the raw UNORM8 values with WARP's 8-bit sub-texel
            // weight, divide by 255 once (the filter WARP uses feeding a float target).
            Filter::Linear => {
                let split = |t: f32| -> (i32, f32) {
                    let c = (t * 65536.0).round_ties_even() as i64 - 0x7f80;
                    ((c >> 16) as i32, ((c >> 8) & 0xff) as f32 / 256.0)
                };
                let (x0, fx) = split(tx);
                let (y0, fy) = split(ty);
                let (c00, c10, c01, c11) = (img.texel(x0, y0), img.texel(x0 + 1, y0), img.texel(x0, y0 + 1), img.texel(x0 + 1, y0 + 1));
                core::array::from_fn(|k| {
                    let a = c00[k] + (c10[k] - c00[k]) * fx;
                    let b = c01[k] + (c11[k] - c01[k]) * fx;
                    (a + (b - a) * fy) / 255.0
                })
            }
        }
    }
}

/// Run `dxbc` once per output pixel and write the result as `R32G32B32A32_FLOAT` (16 B/px, matching
/// the `shader-*` oracle `.bin` — the capture format parity is verified against). `filter`: 0
/// nearest, 1 linear. `extend`: 0 clamp, 1 wrap, 2 mirror. Safe wrapper over the raw pointers so the
/// executor sees ordinary slices.
fn render(dxbc: &[u8], cbuffer: &[u8], input: Option<Img>, out_w: u32, out_h: u32, out: &mut [u8]) -> i32 {
    let shader = match Shader::parse(dxbc) {
        Ok(s) => s,
        Err(_) => return ERR_BAD_DXBC,
    };
    if let Some(op) = shader.unsupported_op() {
        return ERR_UNSUPPORTED_BASE - op as i32;
    }
    if out.len() != out_w as usize * out_h as usize * 16 {
        return ERR_OUT_SIZE;
    }
    let env = ImgEnv(input);
    for y in 0..out_h {
        for x in 0..out_w {
            // SV_Position: pixel centre, z=0, w=1 (the fullscreen-triangle vertex realization).
            let sv = [(x as f32 + 0.5).to_bits(), (y as f32 + 0.5).to_bits(), 0.0f32.to_bits(), 1.0f32.to_bits()];
            let px = PixelInputs { inputs: &[sv], cbuffer };
            let rgba = exec::run(&shader, &env, &px);
            let o = (y as usize * out_w as usize + x as usize) * 16;
            for (k, v) in rgba.iter().enumerate() {
                out[o + k * 4..o + k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
    }
    OK
}

/// C ABI entry the managed host P/Invokes. All buffers are caller-owned; `input_data` may be null
/// (then `input_w`/`input_h` are ignored). Returns an [`OK`]/`ERR_*` status.
///
/// # Safety
/// `dxbc`/`cbuffer`/`input_data`/`out` must point to at least their stated lengths and stay valid
/// for the call; `out_len` must equal `out_w*out_h*16`.
/// The render arguments as one blittable struct. Passing a single pointer keeps the P/Invoke
/// signature to `(ptr) -> int` (Mono-wasm m2n cookie "II"), which every runtime's interp→native
/// table carries — a 13-argument signature does not, and aborted the single-threaded browser
/// interpreter at `aot-runtime-wasm.c:188`. Field order/types match the managed
/// `D2dGpuRenderArgs` (Direct2D1Gpu.cs); keep them in sync.
#[repr(C)]
pub struct D2dGpuRenderArgs {
    pub dxbc: *const u8,
    pub dxbc_len: usize,
    pub cbuffer: *const u8,
    pub cbuffer_len: usize,
    pub input_data: *const u8,
    pub input_w: u32,
    pub input_h: u32,
    pub filter: u32,
    pub extend: u32,
    pub out_w: u32,
    pub out_h: u32,
    pub out: *mut u8,
    pub out_len: usize,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn d2dgpu_render(args: *const D2dGpuRenderArgs) -> i32 {
    if args.is_null() {
        return ERR_BAD_DXBC;
    }
    let a = unsafe { &*args };
    let (dxbc, dxbc_len, cbuffer, cbuffer_len) = (a.dxbc, a.dxbc_len, a.cbuffer, a.cbuffer_len);
    let (input_data, input_w, input_h, filter, extend) = (a.input_data, a.input_w, a.input_h, a.filter, a.extend);
    let (out_w, out_h, out, out_len) = (a.out_w, a.out_h, a.out, a.out_len);
    if dxbc.is_null() || out.is_null() {
        return ERR_BAD_DXBC;
    }
    let dxbc = unsafe { std::slice::from_raw_parts(dxbc, dxbc_len) };
    let cbuffer = if cbuffer.is_null() { &[][..] } else { unsafe { std::slice::from_raw_parts(cbuffer, cbuffer_len) } };
    let out = unsafe { std::slice::from_raw_parts_mut(out, out_len) };
    let input = if input_data.is_null() || input_w == 0 || input_h == 0 {
        None
    } else {
        let n = input_w as usize * input_h as usize * 4;
        let data = unsafe { std::slice::from_raw_parts(input_data, n) };
        if data.len() != n {
            return ERR_INPUT_SIZE;
        }
        Some(Img {
            data,
            w: input_w,
            h: input_h,
            filter: if filter == 1 { Filter::Linear } else { Filter::Nearest },
            extend: match extend {
                1 => Extend::Wrap,
                2 => Extend::Mirror,
                _ => Extend::Clamp,
            },
        })
    };
    render(dxbc, cbuffer, input, out_w, out_h, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../..")
    }

    /// Render a committed `shader-*` scene through the bridge and assert the f32 output is identical
    /// to the WARP capture — the same oracle crates/d2d-shader is validated against. This proves the
    /// bridge's per-pixel driving + input sampling are WARP-exact, not just the executor it wraps.
    fn check(scene_name: &str, max_abs: f32) {
        let root = root();
        let scene: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join(format!("harness/scenes/{scene_name}.json"))).unwrap()).unwrap();
        let w = scene["size"][0].as_u64().unwrap() as u32;
        let h = scene["size"][1].as_u64().unwrap() as u32;
        let mut cbuffer: Vec<u8> = Vec::new();
        if let Some(arr) = scene["cbuffer"].as_array() {
            for x in arr {
                cbuffer.extend_from_slice(&(x.as_f64().unwrap() as f32).to_le_bytes());
            }
            while cbuffer.len() % 16 != 0 {
                cbuffer.push(0);
            }
        }
        let stem = scene["shader"].as_str().unwrap_or(scene_name);
        let dxbc = std::fs::read(root.join(format!("harness/scenes/{stem}.dxbc"))).unwrap();

        let (mut idata, mut iw, mut ih, mut filt, mut ext) = (Vec::new(), 0u32, 0u32, 0u32, 0u32);
        if let Some(i) = scene.get("input").filter(|i| !i.is_null()) {
            iw = i["size"][0].as_u64().unwrap() as u32;
            ih = i["size"][1].as_u64().unwrap() as u32;
            idata = i["rgba"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u8).collect();
            filt = if i["filter"].as_str() == Some("linear") { 1 } else { 0 };
            ext = match i["extend"].as_str() {
                Some("wrap") => 1,
                Some("mirror") => 2,
                _ => 0,
            };
        }
        let mut out = vec![0u8; (w * h) as usize * 16];
        let args = D2dGpuRenderArgs {
            dxbc: dxbc.as_ptr(),
            dxbc_len: dxbc.len(),
            cbuffer: cbuffer.as_ptr(),
            cbuffer_len: cbuffer.len(),
            input_data: if idata.is_empty() { std::ptr::null() } else { idata.as_ptr() },
            input_w: iw,
            input_h: ih,
            filter: filt,
            extend: ext,
            out_w: w,
            out_h: h,
            out: out.as_mut_ptr(),
            out_len: out.len(),
        };
        let status = unsafe { d2dgpu_render(&args) };
        assert_eq!(status, OK, "{scene_name}: render status");

        let cap = std::fs::read(root.join(format!("conformance/oracle/win11-26100/{scene_name}.bin"))).unwrap();
        let want: Vec<f32> = cap.chunks_exact(4).map(|c| f32::from_bits(u32::from_le_bytes([c[0], c[1], c[2], c[3]]))).collect();
        let got: Vec<f32> = out.chunks_exact(4).map(|c| f32::from_bits(u32::from_le_bytes([c[0], c[1], c[2], c[3]]))).collect();
        let worst = want.iter().zip(&got).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst <= max_abs, "{scene_name}: worst abs diff {worst} > {max_abs}");
    }

    #[test]
    fn arith_byte_exact() {
        check("shader-01-arith", 0.0);
    }

    #[test]
    fn flow_byte_exact() {
        check("shader-05-flow", 0.0);
    }

    #[test]
    fn nearest_sample_byte_exact() {
        check("shader-06-sample-nearest", 0.0);
    }

    #[test]
    fn linear_sample_within_one_lsb() {
        // Byte-domain bilinear into a float target: ≤1 LSB alpha residual (phase 3C.1).
        check("shader-06-sample-linear", 1.0 / 256.0 + 1e-6);
    }
}
