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
| **Plug-ins with their own settings window** | A Windows Forms `EffectConfigDialog` or Paint.NET 5 `EffectConfigForm`, including those built on OptionBasedLibrary | Run. paint.rs reads the window and shows its settings in its own dialog, with a live preview: sliders, check boxes, choices, lists, text, colours and the window's buttons. A control the plug-in draws itself (pyrochild's Curves+ curve and Grid Warp grid, Liquify's canvas) shows as the plug-in draws it and is edited there with the mouse, as in Paint.NET; a button that asks for a file opens paint.rs's file chooser. A button's result is kept as Paint.NET keeps it, in history, Repeat and saved images too. |
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

- **Shapes and palettes.** Paint.NET's shape files (`.xaml`) and palette files are not plug-ins here
  and are not supported yet.
- **Plug-in formats save flattened.** A FileType plug-in gets the flattened image, and a layered file
  opens flattened.
- A format a plug-in adds never replaces a built-in one: paint.rs keeps its own WebP, AVIF, JPEG XL,
  DDS, PSD and the rest, and uses the plug-in only for extensions it does not handle. paint.rs opens
  HEIC but cannot save it, so a plug-in that saves HEIC does. The Plugins page in Settings says
  which extensions each plug-in handles and which keep paint.rs's own.
- Some plug-ins call Paint.NET's built-in effects from their own code. paint.rs provides the ones the
  tested packs call (Add Noise, Motion Blur, Zoom Blur, Clouds, Pixelate and others); one it does not
  provide yet stops the plug-in that calls it.
- Plug-ins that draw text or shapes use GDI+ (`libgdiplus` off Windows), whose anti-aliasing differs
  slightly from Windows'.
- **Gaps for the next release.** Midora's EMF and WMF types show their import window, but on Linux and
  macOS the image they open is empty: `libgdiplus` plays back few metafile records, and the plug-in's
  EMF+ conversion reaches into Windows' System.Drawing. Saving EMF is not supported there either.
- **Render pieces.** Paint.NET hands a classic effect the image in tiles of 128 by 128 pixels, and
  paint.rs hands it strips 64 pixels high. Most effects draw the same either way. One that treats each
  piece on its own, such as one that seeds its randomness per piece or marks each piece (madjik's
  ThreadsMarker), shows strips where Paint.NET shows tiles. The next release uses Paint.NET's tiles.
- **Not yet: browsing the ArtCraft Store from inside paint.rs** (it returns in the next release).
  Download items from the [store site](https://akkk09.github.io/artcraft-store/) and install them.
- **Plug-ins that need a Windows program or library**, which paint.rs cannot provide: G'MIC-Qt,
  Camera Raw, PSFilterPdn (Photoshop `.8bf` filters), Koh's ML Effects (ONNX Runtime and its models),
  Median Sketch (Windows' shader compiler), null54's Optimized JPEG (`jpegtran.exe`), JPEG 2000, RIOT
  and Twain, and Midora's WebP and XCF. HEIC FileType Plus needs its own Windows HEIC library (built on
  libheif, for x64 and ARM64 only), so it cannot run in paint.rs; paint.rs opens HEIC files itself.
- **Plug-ins whose point is a Windows feature** paint.rs has no counterpart for: Simon B's Drag From
  (drag and drop onto its window) and Printer+ (printing), TechnoRobbo's FilmStrip and Media plug-in
  (Windows Media Player), and Midora's WhatIsIt and LookAtIt (windows that show information).
- **Open — HTML markup (undecided).** MarkupRenderer draws its output through an embedded web
  browser. The desktop build has no HTML engine, so its output is blank there; the browser build has
  one, so rendering it there is possible. Whether paint.rs should is an open question, not yet decided.
- **`IRenderer<T>` — stand-in shape gap.** The host provides the Paint.NET 3.5.x-era `IRenderer<T>`
  (`Render(ISurface<T>, Int32Point)`) with its `RendererExtensions` and `Document.CreateRenderer()`,
  which serves the 3.x plug-ins that render through it — CurtisBlack.Common, Liquify and ScriptLab all
  work. The Paint.NET 5.x renderer surface — `IRenderer<TPixel>` in `PaintDotNet.Primitives`
  (`Render(RegionPtr<TPixel>, Point2Int32)`) with its 5.x `RendererExtensions`/`CreateRenderer` — is not
  provided; its only consumer is HEIC FileType Plus, which cannot run here regardless (above). The two
  renderer eras coexist in Paint.NET; a future 5.x plug-in other than HEIC that needs the 5.x surface
  would be served by adding it alongside the 3.5.x one, not by replacing it — replacing it would stop
  the three 3.x plug-ins above.

## Platforms

| Platform | Status |
|---|---|
| Linux, Windows, macOS | WebAssembly plug-ins, and Paint.NET's classic, GPU and FileType plug-ins. The release ships its own .NET runtime (the `pdn/` folder beside the program). Linux and macOS also ship OpenD2D and `libgdiplus`; Windows uses its own Direct2D, DirectWrite and GDI+. |
| Browser | WebAssembly plug-ins run as on the desktop. The .NET runtime downloads only once a Paint.NET plug-in is installed. Classic, GPU and FileType plug-ins run as on the desktop: the browser runtime carries its own System.Drawing (text in paint.rs's fonts) and OpenD2D, and keeps the installed files in a folder of its own, so plug-ins find the files beside them. A plug-in that starts its own thread runs it on the page's one thread; the small library that makes this work downloads only when such a plug-in is installed. GPU effects' shaders run in OpenD2D's interpreter there, so heavy ones take noticeably longer than on the desktop. |
| FreeBSD, Linux (musl) | WebAssembly plug-ins. No host for Paint.NET plug-ins yet. |

## When a plug-in fails

A plug-in that cannot load is skipped; paint.rs starts and works without it.

- **Settings ▸ Plugins** lists every installed plug-in with where its effects are in the menus and
  the formats it adds, or, for one that did not load or loaded only in part, the reason (a Paint.NET
  feature paint.rs lacks, a settings window it cannot show, a missing helper library).
- A Paint.NET plug-in that wants to reach outside paint.rs (the network, your files, another program, a
  system setting such as the desktop wallpaper, or native code) asks first: "Upload.dll wants to connect to
  imgur.com", with Allow once, Always allow and Deny. Deny is the default; a denied plug-in gets the error
  it would get offline and carries on. Always allow answers are listed under the plug-in on the Plugins page,
  each with Revoke.
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

## Permissions

A Paint.NET plug-in's job is to change the image. When one instead tries to reach **outside** the
image, paint.rs asks you first and does nothing until you allow it. The things it asks about:

- **Network** — connecting to the internet (uploading the image, fetching something).
- **Files** — reading or writing files of its own on your computer.
- **Running a program** — starting another process.
- **System settings** — changing something outside paint.rs (the registry, environment).
- **Native code** — loading a native library that runs outside the managed runtime.

Each request names the plug-in and what it wants (for the network, the address). You can **Allow once**,
**Always allow** that plug-in that thing, or **Deny** — and a denied plug-in sees the same failure it
would with no internet or no file, so one written to cope keeps working. Change your mind any time in
**Settings ▸ Plugins**, where each plug-in's granted permissions are listed and can be revoked; revoking
takes effect on the plug-in's next run. Reading and writing the **clipboard** is not asked about —
plug-ins use it freely, as they do in Paint.NET.

**In the browser this is a hard wall.** A plug-in reaches the network only through this gate (its one
network path, `fetch`, is held by paint.rs; a request you allow, paint.rs makes for it and hands back
the reply). Files, running a program and system settings do not exist in a browser, so a plug-in cannot
touch them at all, and a plug-in that tries to call the browser's own code directly (JavaScript interop)
is refused when it loads. Nothing a plug-in does on the page escapes what you have allowed.

**On the desktop the prompt covers the same five**, but it is not yet a hard boundary: a plug-in runs
with the app's permissions, as it does in Paint.NET, and one using native code can step around the gate.
A real sandbox that closes this comes in the next release (0.8.1). Until then, treat the desktop gate as
a guard against mistakes and ordinary plug-ins, not against a hostile one — **install plug-ins only from
authors you trust.** WebAssembly plug-ins are not affected: they run in the sandbox described above, with
no host access at all.
