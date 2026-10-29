# Plug-ins

paint.rs runs **PhotoCraft's WebAssembly filter plug-ins** (plug-in ABI v1) and installs
**LightCraft looks and develop presets**. Installed items appear under **Effects ▸ Plugins**,
marked with a puzzle piece.

## Installing

- **Settings ▸ Plugins ▸ Browse Store…** lists the PhotoCraft and LightCraft items of the
  [ArtCraft Store](https://akkk09.github.io/artcraft-store/) (its catalog is fetched each time) and
  installs one in a click.
- **Settings ▸ Plugins ▸ Install…**, or drop a file on the window: `.wasm` (a PhotoCraft plug-in),
  `.cube` or `.3dl` (a look), `.xmp` (a Camera Raw develop preset).
- On the desktop, installed files live in the `plugins` folder of the app's data folder (Settings ▸
  Plugins shows where); files you copy there yourself load at the next start. On the web they live
  in the browser's storage.

A plug-in's dialog is built from the settings it declares, with a live preview; plug-ins work with
the selection, undo, Repeat and the images picked in the top strip, like the built-in effects. A
look has an Amount slider; a preset applies Camera Raw's settings at once (Camera Raw's own **Load
Preset…** loads an `.xmp` into its dialog instead).

## The sandbox

A plug-in is a WebAssembly module run by paint.rs's built-in interpreter
([wasmi](https://github.com/wasmi-labs/wasmi)), so the same file runs on the desktop and in the
browser. It gets **no host imports**: no files, network, clock or randomness, and a module that
imports anything is refused. Each run has an instruction budget, a 5-minute time limit (desktop)
and a 1 GiB memory cap, and runs in a fresh instance. The image is handed over in one call when it
fits (up to about 4K), else in tiles. A plug-in that traps, loops or returns garbage leaves the
image as it was and reports why; Cancel stops a long one.

## Writing a plug-in

The ABI is PhotoCraft's (`crates/plugins` in [PhotoCraft](https://github.com/storytold/photocraft),
MIT), so a plug-in works in both apps. A module exports its linear memory as `memory` and four
functions:

| Export | Does |
|---|---|
| `pc_abi_version() -> i32` | Returns `1`. |
| `pc_manifest() -> i64` | Returns `(len << 32) \| ptr` of a UTF-8 JSON manifest in its memory. |
| `pc_alloc(size: i32) -> i32` | Returns a block of `size` bytes in its memory (0 when out of memory). |
| `pc_filter(buf, buf_len, width, height, channels, format, params, params_len) -> i32` | Edits the pixels in place; 0 on success, any other value is an error code. |

**The manifest:**

```json
{
  "id": "org.example.warm-tint",
  "name": "Warm Tint",
  "version": "1.0.0",
  "kind": "filter",
  "author": "You",
  "description": "Pulls colours toward amber.",
  "params": {
    "amount": {"type": "int", "min": 0, "max": 100, "default": 40},
    "strength": {"type": "number", "min": 0, "max": 1, "default": 0.5},
    "keepLight": {"type": "bool", "default": true},
    "mode": {"type": "choice", "options": ["soft", "hard"], "default": "soft"}
  },
  "overlap": 0,
  "area": "content"
}
```

- `id`: 1–64 characters from `A-Z a-z 0-9 . _ -` (reverse-DNS style is recommended). Installing a
  plug-in with the same id replaces it.
- `params`: in dialog order. `int` and `number` are sliders (`"unit": "px"` marks a length in
  pixels), `bool` a check box, `choice` a drop-down.
- `overlap`: pixels of context your filter needs around each output pixel (0 for a per-pixel
  filter; at most 256). Tiles of a large image are handed over with that margin, taken from the
  neighbouring pixels.
- `area`: `content` (the layer's visible pixels, the default) or `canvas` (the whole canvas,
  transparent pixels too, for something like Clouds).

**The pixels** (`buf`, `buf_len` bytes): interleaved `f32` samples, 0–1, straight alpha. paint.rs
hands over RGBA, so `channels` is 4 and `format` is `0x30108` (bits 0–7 the source depth, 8; bit 8
alpha; bits 16–23 the colour mode as numbered in the PSD format, 3 for RGB). NaN or infinite output
keeps the original sample, and output is clamped to 0–1.

**The parameters** (`params`, `params_len` bytes): a JSON object of every declared parameter
(validated, defaults filled in, numbers clamped), plus `"_image"`: the buffer's `x`, `y`, `width`,
`height` and `overlap` and the `canvasWidth` and `canvasHeight`. A filter that depends on the
position in the image (a vignette, a rotation) should place itself by these: a very large image
arrives in tiles.

**A complete plug-in in Rust** (`Cargo.toml` with `crate-type = ["cdylib"]`, built with
`cargo build --release --target wasm32-unknown-unknown`):

```rust
//! Warm Tint: pulls every pixel's colour toward amber by `amount`, keeping alpha.
#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

static MANIFEST: &str = r#"{
  "id": "org.example.warm-tint",
  "name": "Warm Tint",
  "version": "1.0.0",
  "kind": "filter",
  "author": "You",
  "description": "Pulls colours toward amber.",
  "params": {
    "amount": {"type": "int", "min": 0, "max": 100, "default": 40},
    "keepLight": {"type": "bool", "default": true}
  }
}"#;

#[unsafe(no_mangle)]
pub extern "C" fn pc_abi_version() -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn pc_manifest() -> i64 {
    ((MANIFEST.len() as i64) << 32) | MANIFEST.as_ptr() as i64
}

// A bump allocator that starts after the module's own stack and data (`__heap_base`), so the
// buffers the host asks for never overwrite them.
unsafe extern "C" {
    static __heap_base: u8;
}
static mut NEXT: usize = 0;

#[unsafe(no_mangle)]
pub extern "C" fn pc_alloc(size: i32) -> i32 {
    unsafe {
        if NEXT == 0 {
            NEXT = &raw const __heap_base as usize;
        }
        let start = (NEXT + 7) & !7;
        let end = start + size.max(0) as usize;
        let pages = end.div_ceil(65536);
        let have = core::arch::wasm32::memory_size(0);
        if pages > have && core::arch::wasm32::memory_grow(0, pages - have) == usize::MAX {
            return 0;
        }
        NEXT = end;
        start as i32
    }
}

/// `"key": <number>` from the params JSON.
fn number(params: &[u8], key: &str) -> Option<f32> {
    let text = core::str::from_utf8(params).ok()?;
    let at = text.find(key)? + key.len();
    let rest = text[at..].trim_start().strip_prefix(':')?.trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn pc_filter(
    buf: i32,
    len: i32,
    _width: i32,
    _height: i32,
    channels: i32,
    _format: i32,
    params: i32,
    params_len: i32,
) -> i32 {
    if channels != 4 {
        return 1;
    }
    let pixels = unsafe { core::slice::from_raw_parts_mut(buf as *mut f32, len as usize / 4) };
    let params = unsafe { core::slice::from_raw_parts(params as *const u8, params_len as usize) };
    let k = number(params, "\"amount\"").unwrap_or(40.0) / 100.0;
    let keep_light = !params.windows(17).any(|w| w == b"\"keepLight\":false");
    for p in pixels.chunks_exact_mut(4) {
        let lum = 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
        let light = if keep_light { lum.max(0.2) } else { 1.0 };
        for (c, amber) in [1.0, 0.75, 0.4].into_iter().enumerate() {
            p[c] += (amber * light - p[c]) * k;
        }
    }
    0
}
```

The allocator matters: the ArtCraft Store's template starts its heap at 64 KiB, inside the 1 MiB
stack Rust places first, so a plug-in that copies a large buffer to scratch can overwrite its own
statics. paint.rs reserves the module's initial memory with one `pc_alloc` call before anything
else, which keeps those plug-ins working, but starting at `__heap_base` (or using a real
allocator) is correct in any host.

Drop the `.wasm` on paint.rs to install it, and choose **Effects ▸ Plugins ▸ Warm Tint**.
