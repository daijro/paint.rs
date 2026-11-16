# Paint.NET plugins in the browser

The browser half of the Paint.NET host: the .NET `browser-wasm` runtime (Mono interpreter)
loads an uploaded plugin assembly from bytes at run time and drives it over a pixel buffer
handed in from the page. It reuses the desktop host and replacement PaintDotNet assemblies —
it does **not** re-implement them: `app/Host.cs` is a thin `[JSExport]` wrapper over the same
`PluginHost` the desktop bridge drives (`pdn-host/bridge`), so an effect renders **byte-for-byte
identically** on both targets.

```
app/            net10.0 browser-wasm host: Host.cs (PdnWebHost JSExport bridge), wwwroot/{index,main}.js
drawing/        System.Drawing.Common without GDI+, for plug-ins' icons, drawing, text and images
testplugin/     a stand-in (BGRA invert); superseded as the asserted test by the real effect below
test/verify.mjs headless-Chromium check, node built-ins only (no puppeteer)
test/census.mjs which plug-ins load and run in headless Chromium, and the System.Drawing their failures name
stage.sh        publish + stage the AppBundle into a paint.rs web site's pdn/ folder
```

## The ABI (same as the desktop C ABI, `pdn-host/docs/pdn-bridge.md`)

`PdnWebHost` exposes, through `[JSExport]`, the same operations the desktop reaches through
hostfxr function pointers, over the same JSON payloads:

- `LoadPlugin(byte[]) → int` — load a plugin assembly from uploaded bytes; returns a handle.
- `Enumerate(handle) → json` — the effects and FileTypes the plugin provides.
- `EffectSchema(handle, id) → json` — one effect's property schema.
- `Render(handle, id, props, bgra, w, h, x, y, rw, rh) → bgra` — render one effect in place over
  a BGRA8 buffer; `(x,y,rw,rh)` is the region of interest.
- `FreePlugin(handle)`.

The managed `PluginHost`/`FileTypeDriver` are **compiled in** from `pdn-host/bridge`, referencing
the one unified replacement set (phase 5A-4) — the same assemblies `PdnBridge.csproj` references.
A plugin arrives with no folder, so `PluginHost.LoadBytes` loads it from a `MemoryStream`; its
`PaintDotNet.*` identities resolve from the host context (the replacement set), exactly as on the
desktop.

## What runs today (verified headless, `test/verify.mjs` → PASS)

- The runtime loads **on demand**: the page downloads nothing but `main.js` until `pdnweb.loadPlugin()`
  is first called (confirmed `booted() === false` before use).
- A real classic effect — `PdnSelfTest.InvertEffect`, a `PaintDotNet.Effects.PropertyBasedEffect`,
  the same effect the desktop selftest renders — is loaded from uploaded bytes by the Mono
  interpreter and driven through `SetRenderInfo`+`Render`. Its output over a BGRA8 buffer (whole
  image **and** a region of interest) is **byte-exact** against the desktop reference.
- Enumerate and the property schema cross the boundary as the same JSON the desktop emits.
- A **FileType** save→open round-trip is byte-exact for a pure-managed in-tree FileType
  (`RawFileType`), through the same `FileTypeDriver` the desktop uses; the web wires it into
  paint.rs's Open/Save (`io::plugins`) with no target-specific call site.
- Render is **incremental and cancellable** in the browser: the ROI's strips render a few at a
  time across frames (like the seam carver), so Cancel and progress work on the single-threaded
  runtime. The stepped result is byte-identical to a one-shot render (verified).
- Real third-party plugins load from bytes in-browser: `WebPFileType.dll` enumerates its FileType;
  `DdsFileTypePlus.dll` loads but needs a replacement type the unified set does not carry yet
  (`PaintDotNet.Imaging.BitmapSourceBase<T>`) — widening coverage means adding that type, not
  changing the host.

**System.Drawing** has no GDI+ to call here, and the package throws on every GDI+ call off Windows,
yet plug-ins build their icons, draw shapes and text, and read images with it. The host therefore
carries its own System.Drawing.Common (`drawing/`, see its README): the package's API, with what
plug-ins use implemented in managed code. Its text and JPEG/TIFF/WebP decoding come from paint.rs
(`pdnBoot` hands them in). On the desktop bridge with it in place of libgdiplus, the six plug-in
packs load and run as they do on the desktop (`drawing/README.md` has the numbers).

## paint.rs integration

`pdn_core::effect::dotnet` has a `wasm` path that mirrors the desktop one over this host, through
`crates/pdn-core/src/effect/pdn_web.js` (wasm-bindgen): async boot, then synchronous
load/enumerate/schema/render, so a plugin effect runs through the same session/dialog/undo pipeline
as every built-in. Installed `.dll` files live in the browser's IndexedDB
(`crates/paintrs/src/platform/plugin_web.{rs,js}`, `PluginStore::Browser`) and load at start; the
`.NET` runtime download is paid only when a plugin is installed, and its size is reported in
Settings ▸ Plugins. Stage the host into a built site with `./stage.sh dist` (it lands at `dist/pdn/`).

## Download size (on demand, this untrimmed bundle)

~25 MiB transferred raw on first run (the whole bundle loads eagerly at boot); ~8.5 MiB gzip, less
over Brotli (what a real server, and `stage.sh`'s `.gz`, send). Trimming is off on purpose: plugins
arrive after publish, so the trimmer cannot see which BCL members they call. The lever to shrink the
download is trimming with the compat assemblies + common BCL rooted, or a per-plugin trimmed build.

## Build & verify

Prereqs: a .NET 10 SDK **with the `wasm-tools` workload**, and a Chromium binary. This workstation
has no system workload and `/usr/share/dotnet` is read-only, so a **user-local SDK** was used:

```sh
curl -fsSL https://dot.net/v1/dotnet-install.sh | bash -s -- --channel 10.0 --install-dir ~/.dotnet
~/.dotnet/dotnet workload install wasm-tools          # pulls the Mono browser-wasm runtime + emscripten
export DOTNET_ROOT="$HOME/.dotnet" PATH="$HOME/.dotnet:$PATH"
```

Then:

```sh
dotnet build   paintnet/tests/selftest/selftest.csproj -c Release -o /tmp/tp   # the in-tree effect
dotnet publish app/PaintNetWeb.csproj                  -c Release
node test/verify.mjs app/bin/Release/net10.0/browser-wasm/AppBundle /tmp/tp/PdnSelfTest.dll \
     [extra-plugin.dll ...]                            # extras load in report-only mode
```

(The host build goes through `paintnet/dn.sh`/`bridge/dn.sh`, which keep the SDK/NuGet caches inside
`.build/`.)

## Remaining

- **Native-codec FileType/effect plugins** (e.g. WebP's `libwebp`, DDS's codec) load and enumerate
  but cannot decode/encode until their native codec is linked into the wasm runtime — the same
  `<NativeFileReference>` relink the GPU seam needs (plan below).
- **Imaging replacement types** some plugins reference (`PaintDotNet.Imaging.BitmapSourceBase<T>`
  and its cone) are not in the unified set yet, so e.g. `DdsFileTypePlus` fails to enumerate. These
  are shared with the desktop FileType work (AVIF/JXL) and belong on the desktop branch.
- **Official plugins target `net11.0`** while this runtime is `net10`; our ALC redirects
  `PaintDotNet.*`/`System.*` by simple name, so many pre-net11 plugins load, but the net11 runtime
  pack is the clean target for the official corpus.
- **GPU effects** run on opend2d's Direct2D/DirectWrite built for wasm, except text (see below).

## GPU effects in the browser

GPU plugins (`GpuImageEffect`, `GpuDrawingEffect`, ComputeSharp.D2D1 pixel shaders) run on the desktop
through the replacement `PaintDotNet.Direct2D1`/`DirectWrite` wrappers over opend2d's `libd2d1`/`libdwrite`
COM objects (`pdn-host/README.md`). The browser links the same drop-ins into the Mono wasm runtime:
`build-native.sh` builds opend2d's d2d-ffi (which carries dwrite-ffi) for `wasm32-unknown-emscripten` as
`d2d1.a`, plus an empty `dwrite.a` that only names the module (Mono-wasm keys its P/Invoke table by archive
file name, and a second Rust archive would duplicate std). `PaintNetWeb.csproj` links both with
`<NativeFileReference>`; the publish relinks them into `dotnet.native.wasm`, and the P/Invoke table maps
`d2d1` → `D2D1CreateFactory` and `dwrite` → `DWriteCreateFactory`. Everything else goes through COM
vtables, as on the desktop. `GpuDriver` compiles into the web host unchanged.

**Status: GPU plug-ins render in the browser** (headless Chromium, `test/census.mjs`). Two kinds of call
the relink cannot know about, because plug-ins arrive after publish, are made at run time:

- *Native code calling plug-in code.* The relink makes entry points only for the `[UnmanagedCallersOnly]`
  methods in the bundled assemblies. GPU plug-ins carry their own copy of ComputeSharp's effect interop,
  whose COM objects OpenD2D calls back into.
- *Plug-in code calling native code.* The relink makes interpreter invokers only for the DllImport
  signatures in the bundle. Plug-ins call COM methods through function pointers.

`app/native/pdn-trampolines.c` fills both gaps. When the runtime's own tables have nothing, it adds a JS
function to the wasm table (Emscripten's `addFunction`; the runtime already links with table growth) that
does what a generated entry point or invoker does. It is made once per method or signature, for any wasm
signature over i32, i64, f32 and f64. A type the wasm ABI cannot pass that way fails only that call, as a
managed exception. `PaintNetWeb.csproj` compiles the runtime pack's `runtime.c` with it, patching the
three lines that hook it in (the build stops if a pack no longer has them). It also always links Mono's
marshal-ilgen, which the relink otherwise includes only when the bundle itself needs it.

GPU text uses paint.rs's own faces. `pdnBoot` writes them into the runtime's file system at `/fonts`, sets
`D2D_FONTS` there, and names Selawik (paint.rs's Segoe UI) as `D2D_FALLBACK_FAMILY` for families none of them has.
OpenD2D's fallback otherwise asks for Segoe UI itself, so Level Horizon and Meme Maker need OpenD2D to honour that
variable. Plug-ins that start threads stay desktop-only, because the runtime has no threads.

Toolchain details (Rust 1.99's LLVM vs the .NET 10 wasm pack, emscripten 3.1.56): the archive is built
`-Ctarget-cpu=mvp`, and the relink's `wasm-opt` must be binaryen ≥ 118 (the pack ships 117, which rejects
the feature flags Rust's LLVM stamps; put binaryen 133 on the Sdk pack's `bin/`, keeping `wasm-opt.orig`).
The net11 route is blocked by an RC-SDK bug (its browser-wasm publish does not resolve the Mono runtime
pack); pin net10 with a `global.json` (`{"sdk":{"version":"10.0.403"}}`) while both SDKs are installed.

### Build steps

Prereqs: the wasm-tools workload (above), `rustup target add wasm32-unknown-emscripten`, a `git worktree
add` of opend2d main, and binaryen ≥ 118 as above. Then:

```sh
./build-native.sh <opend2d-worktree>                       # -> .build/native-wasm/.../{d2d1,dwrite}.a
./../paintnet/dn.sh publish app/PaintNetWeb.csproj -c Release   # links them when present
```

The relink's `MarshalingPInvokeScanner` runs in an MSBuild task host, which this workstation's sandbox
cannot start (it blocks the task host's pipe); run the publish outside it.

### Notes

- **Same relink unblocks native-codec FileType plugins.** With `<NativeFileReference>` + relink
  working, a FileType plugin that P/Invokes a native codec (e.g. WebP's `libwebp`) works too by adding
  that codec's wasm archive — the real WebP round-trip now has a path.
- Size: with the Direct2D/DirectWrite drop-ins linked, `dotnet.native.wasm` is 6.9 MB (uncompressed),
  behind the on-demand boot, so only plugin users pay it. A per-plugin trimmed build is the lever if it
  matters.
