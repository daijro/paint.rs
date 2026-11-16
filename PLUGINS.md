# Plug-ins

paint.rs runs two kinds of plug-ins:

- **Paint.NET plug-ins**: the same `.dll` files that extend Paint.NET on Windows. They run on an
  embedded .NET runtime against paint.rs's own replacement for Paint.NET's plug-in API, so a plug-in
  sees paint.rs's image as it would see Paint.NET's. Effects and adjustments appear under
  **Effects ▸ Plugins**, in the submenu a plug-in names for them (Effects ▸ Plugins ▸ Artistic ▸ …) or directly
  there when it names none, and FileType plug-ins add formats to **Open** and **Save**.
- **WebAssembly plug-ins**: PhotoCraft filters (`.wasm`), LightCraft looks (`.cube`, `.3dl`) and
  Camera Raw develop presets (`.xmp`). They appear under **Effects ▸ Plugins** too, marked with a
  puzzle piece. See [WebAssembly plug-ins](#webassembly-plug-ins).

## Installing and removing

- **Settings ▸ Plugins ▸ Install…**, or drop the files on the window: a plug-in `.dll` or `.wasm`, a
  look (`.cube`, `.3dl`), a preset (`.xmp`), or a `.zip` holding them (a pack's zip installs every
  plug-in in it). Helper libraries a plug-in ships beside it
  (`AForge.dll` and the like) install the same way, and so do the files some plug-ins read from
  beside themselves: the `.dlc` captions of toe_head2001's plug-ins, the `.json` presets of Koh's.
  Install the whole zip.
- Plug-ins load as soon as they are installed; there is no restart.
- On the desktop the files live in the `plugins` folder of the app's data folder (Settings ▸ Plugins
  shows where). Files you copy there yourself load at the next start. In the browser they are kept
  in the site's storage and load when the page opens.
- **Remove** a plug-in from Settings ▸ Plugins, or delete its file from the folder.

## What runs

| Kind | What it is | Status |
|---|---|---|
| **Classic effects and adjustments** | CPU `Effect`s, including the Paint.NET 3.x and 4.x plug-ins most forum packs are | Run. Their property dialogs (IndirectUI) are built from the plug-in's properties, with a live preview. |
| **Plug-ins with their own settings window** | A Windows Forms `EffectConfigDialog` or Paint.NET 5 `EffectConfigForm`, including those built on OptionBasedLibrary | Run. paint.rs reads the window and shows its settings in its own dialog, with a live preview: sliders, check boxes, choices, lists, text, colours and the window's buttons. A button's result is kept as Paint.NET keeps it, in history, Repeat and saved images too. |
| **GPU effects** | Paint.NET 5's `GpuImageEffect`, `GpuDrawingEffect` and the CPU `BitmapEffect`, with ComputeSharp pixel shaders | Run through OpenD2D, paint.rs's implementation of Direct2D and DirectWrite, on the desktop and in the browser. |
| **FileType plug-ins** | Paint.NET 5.2's file types and the classic ones (`FileType`, `PropertyBasedFileType`) | Their formats are listed in Open and Save. A format's save options come from the plug-in's properties and show in the Save dialog. A window a file type shows of its own while it opens or saves (TechnoRobbo's Icons and Animated Cursors, Midora's metafile import) shows in paint.rs's, as an effect's does. |

Plug-ins see the app's primary and secondary colours and the Colors window's palette, the selection's
shape, the clipboard's image, and the built-in file types (Fill From File loads PNG, JPEG, BMP, GIF and
TIFF through them).

Tested on the most used packs from the Paint.NET forum's Plugin Index and their authors' own pages:
- BoltBait's packs (the classic one and the GPU one) and Paint.NET's own PdnV5EffectSamples;
- pyrochild's, Ed Harvey's, Red ochre's, MadJik's, toe_head2001's, dpy's, KrisVDM's, Simon B's,
  EER's, ReMake's and Tanel's packs;
- null54's pack and plug-ins (Paint Shop Pro, ABR brushes, Pattern, Tile Image, Base64, Content-Aware
  Fill);
- Koh's GPU effects and Rick Brewster's Iris Blur;
- TechnoRobbo's, MJW's, Xhin's, Reptillian's and Midora's plug-ins.

On the desktop 543 of their 550 effects run, and in the browser 540 of the 541 it lists. The rest are
listed below.

## Known limitations

- **Drawn controls in settings windows.** A curve in a plug-in's window (pyrochild's Curves+) is
  edited in paint.rs's own curve editor, and a displacement grid (pyrochild's Grid Warp) in its own grid
  editor over the image. Liquify's and Smudge's brush canvas is not adjustable yet; those effects run
  with the rest of their settings. A window's buttons that open a file (Load, Save, Browse) are not
  offered.
- **Shapes and palettes.** Paint.NET's shape files (`.xaml`) and palette files are not plug-ins here
  and are not supported yet.
- **Plug-in formats save flattened.** A FileType plug-in gets the flattened image, and a layered file
  opens flattened.
- A format a plug-in adds never replaces a built-in one: paint.rs keeps its own WebP, AVIF, JPEG XL,
  DDS, PSD and the rest, and lists the plug-in only for extensions it does not handle.
- Some plug-ins call Paint.NET's built-in effects from their own code. paint.rs provides the ones the
  tested packs call (Add Noise, Motion Blur, Zoom Blur, Clouds, Pixelate and others); one it does not
  provide yet stops the plug-in that calls it.
- Plug-ins that draw text or shapes use GDI+ (`libgdiplus` off Windows), whose anti-aliasing differs
  slightly from Windows'.
- **Gaps for the next release.** Midora's EMF and WMF types show their import window, but on Linux and
  macOS the image they open is empty: `libgdiplus` plays back few metafile records, and the plug-in's
  EMF+ conversion reaches into Windows' System.Drawing. Saving EMF is not supported there either.
- **Not yet: browsing the ArtCraft Store from inside paint.rs** (it returns in the next release).
  Download items from the [store site](https://akkk09.github.io/artcraft-store/) and install them.
- **Plug-ins that need a Windows program or library**, which paint.rs cannot provide: G'MIC-Qt,
  HEIC FileType Plus, Camera Raw, PSFilterPdn (Photoshop `.8bf` filters), Koh's ML Effects (ONNX
  Runtime and its models), Median Sketch (Windows' shader compiler), null54's Optimized JPEG
  (`jpegtran.exe`), JPEG 2000, RIOT and Twain, and Midora's WebP and XCF.
- **Plug-ins whose point is a Windows feature** paint.rs has no counterpart for: Simon B's Drag From
  (drag and drop onto its window) and Printer+ (printing), TechnoRobbo's FilmStrip and Media plug-in
  (Windows Media Player), and Midora's WhatIsIt and LookAtIt (windows that show information).

## Platforms

| Platform | Status |
|---|---|
| Linux, Windows, macOS | WebAssembly plug-ins, and Paint.NET's classic, GPU and FileType plug-ins. The release ships its own .NET runtime (the `pdn/` folder beside the program). Linux and macOS also ship OpenD2D and `libgdiplus`; Windows uses its own Direct2D, DirectWrite and GDI+. |
| Browser | WebAssembly plug-ins run as on the desktop. The .NET runtime downloads only once a Paint.NET plug-in is installed. Classic, GPU and FileType plug-ins run as on the desktop: the browser runtime carries its own System.Drawing (text in paint.rs's fonts) and OpenD2D, and keeps the installed files in a folder of its own, so plug-ins find the files beside them. A plug-in that starts its own thread runs it on the page's one thread; the small library that makes this work downloads only when such a plug-in is installed. GPU effects' shaders run in OpenD2D's interpreter there, so heavy ones take noticeably longer than on the desktop. |
| FreeBSD, Linux (musl) | WebAssembly plug-ins. No host for Paint.NET plug-ins yet. |

## When a plug-in fails

A plug-in that cannot load is skipped; paint.rs starts and works without it.

- **Settings ▸ Plugins** lists every installed plug-in that did not load, or loaded only in part,
  with the reason (a Paint.NET feature paint.rs lacks, a settings window it cannot show, a missing
  helper library).
- When paint.rs starts, one notice says which plug-ins did not load.
- An effect that stops with an error while it runs leaves the image unchanged and says so.

## WebAssembly plug-ins

A plug-in's dialog is built from the settings it declares, with a live preview; plug-ins work with
the selection, undo, Repeat and the images picked in the top strip, like the built-in effects. A
look has an Amount slider; a preset applies Camera Raw's settings at once (Camera Raw's own **Load
Preset…** loads an `.xmp` into its dialog instead).

### The sandbox

A plug-in is a WebAssembly module run by paint.rs's built-in interpreter
([wasmi](https://github.com/wasmi-labs/wasmi)), so the same file runs on the desktop and in the
browser. It gets **no host imports**: no files, network, clock or randomness, and a module that
imports anything is refused. Each run has an instruction budget, a 5-minute time limit (desktop) and
a 1 GiB memory cap (a `memory.grow` past it traps), and runs in a fresh instance. The image is
handed over in one call when it fits (up to about 4K), else in tiles. A plug-in that traps, loops or
returns garbage leaves the image as it was and reports why; Cancel stops a long one.

### Writing a plug-in

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

The allocator matters: a plug-in whose heap starts at 64 KiB sits inside the 1 MiB stack Rust places
first, so copying a large buffer to scratch can overwrite its own statics. paint.rs reserves the
module's initial memory with one `pc_alloc` call before anything else, which keeps those plug-ins
working, but starting at `__heap_base` (or using a real allocator) is correct in any host.

Drop the `.wasm` on paint.rs to install it, and choose **Effects ▸ Plugins ▸ Warm Tint**.

## Trust

Paint.NET plug-ins are programs and run with the app's permissions, as they do in Paint.NET. Install
them only from authors you trust. WebAssembly plug-ins run in the sandbox described above.
