# pdn-host — hosting real Paint.NET plugins in paint.rs

This tree is the Paint.NET **plugin hosting layer**: an embedded .NET runtime, one
unified set of independent replacement `PaintDotNet.*` assemblies, and the host code
that loads an official Paint.NET plugin DLL and drives its effects / FileTypes over
paint.rs buffers.

## Provenance

Copied (plain copy, no git history) from **opend2d** `hosts/` at commit
`05469b31755ee1b50ca73326601c83562158427d`. opend2d stays a pure Direct2D /
DirectWrite / WIC drop-in with a 1:1 interface; the hosting layer now lives here and
paint.rs owns it. opend2d is consumed only as the graphics libraries (for GPU plugins,
later). **Not** copied: `ref/` (the genuine Paint.NET assemblies and bundled plugins —
redistribution is forbidden), any Paint.NET binaries, and all build output.

The replacement assemblies under `paintnet/assemblies/` are fresh, idiomatic C# written
against the *public metadata* of the reference build — no decompiled Paint.NET code is
copied. They carry the real assembly identities/versions so a plugin's IL binds here
unchanged and also binds to the genuine assemblies on Windows (that dual bind is what
lets the output be verified byte-identical). They are therefore safe to ship. Never
commit or ship anything from opend2d's `ref/`.

## Layout

| Dir | What | Status in paint.rs |
| --- | --- | --- |
| `paintnet/` | The ONE unified replacement assembly set (`assemblies/`) + the classic CPU effect host (`host/`, pure reflection) + the in-tree Invert selftest | **builds; selftest PASS** |
| `paintnet-filetypes/` | FileType plugin host (`IFileTypeFactory`, load/save over a flat BGRA document) + the Linux native codec wrappers (`native/`) | **host builds**; native WebP wrapper + round-trip test not yet rebuilt here |
| `paintnet-gpu/` | The earlier single-shader GPU prototype (d2dgpu bridge); its `plugins/SampleGpuEffect` is now the in-tree test plugin for the Paint.NET 5 GPU API (`crates/pdn-bridge/tests/gpu.rs`) | **reference only** — GPU effects run through the replacement `PaintDotNet.Direct2D1` wrappers over opend2d's `libd2d1` (below) |
| `paintnet-web/` | Browser host (.NET `browser-wasm` / Mono). The later web-port agent implements the same ABI here | **reference only — deferred** |
| `bridge/` | The C-ABI plugin host built on this set, and its Rust binding in paint.rs | *(added by the 0.8.0 .NET-host work)* |

The C-ABI host and its JSON schema are specified in `docs/pdn-bridge.md`.

## Building

Each host has a `dn.sh` that redirects every SDK/NuGet cache into its gitignored
`.build/` (this dev sandbox makes `~/.dotnet`, `~/.nuget` and `/run/user` read-only).

**Always pass `-m:1`.** This Arch `dotnet` SDK (10.0.113) has a broken workload
install; the `Microsoft.NET.SDK.WorkloadAutoImportPropsLocator` SDK resolver errors
(`MSB4276`), and in a *parallel* multi-project restore graph walk that error turns
fatal (a silent "Build FAILED, 0 errors"). Serializing to one node avoids it, which is
why the upstream `run.py` already passes `-m:1`. Builds also need
`--disable-build-servers -nodeReuse:false` (the sandboxed MSBuild node server cannot
complete its handshake).

```sh
# classic CPU effect host + the full assembly set, then the Invert selftest
cd paintnet
./dn.sh build assemblies/Effects.Core/Effects.Core.csproj -o .build/shim -m:1 --disable-build-servers -nodeReuse:false
./dn.sh build host/pdnhost.csproj                          -o .build/host -m:1 --disable-build-servers -nodeReuse:false
python3 tests/run.py selftest            # -> "selftest PASS"

# FileType host (compiles the Windows/WindowsCore/FileTypes subtree too)
cd ../paintnet-filetypes
./dn.sh build host/host.csproj -c Release -m:1 --disable-build-servers -nodeReuse:false
```

A first build restores the ref packs from nuget.org (one online restore; later builds
are offline). Official Paint.NET 5.2 plugins target **net11**, so the bridge (`bridge/`)
targets net11 and needs the .NET 11 SDK; the replacement assemblies stay net10, which a
net11 runtime loads (and so does the net10 web host).

## Packaging the runtime

`bridge/publish.sh` publishes the host into an app-local folder that paint.rs finds via
`$PDN_BRIDGE_DIR` (development) or a `pdn/` folder beside the executable (release). It builds
opend2d's native libraries from `$OPEND2D_DIR`, which must be set (there is no default; CI sets it
with `scripts/ci/fetch-opend2d.sh`).

| Mode | Command | Size | Needs |
| --- | --- | ---: | --- |
| Framework-dependent | `./publish.sh <out>` | **2.3 MB** | the .NET 11 runtime installed on the machine |
| Self-contained | `./publish.sh <out> linux-x64` | **82 MB** / RID | nothing — the runtime is bundled |

Development uses framework-dependent against the installed SDK (set `PDN_BRIDGE_DIR` to
`bridge/bin/Release/net11.0` or a published folder). Releases that cannot assume .NET ship
self-contained per platform RID (`linux-x64`, `win-x64`, `osx-arm64`, …). Trimming is left
**off**: plugin IL is loaded at run time, so the trimmer cannot see which BCL members a
plugin will call (the same constraint as the web bundle). A trimmed build with the
replacement set + common BCL rooted is the lever to shrink the 82 MB, once the plugin
surface is pinned.

### Where `pdn/` goes in each release bundle

paint.rs locates the host at `$PDN_BRIDGE_DIR`, else a `pdn/` folder **beside the
executable** (`platform/plugins.rs::bridge_dir`). So each `.github/workflows/release.yml`
desktop job publishes the host self-contained for its RID and drops it next to the binary:

| Job | `publish.sh` RID | `pdn/` lands at |
| --- | --- | --- |
| linux (tarball) | `linux-x64` / `linux-arm64` | `<tarball root>/pdn/` (beside `paintrs`) |
| linux (AppImage) | same | `AppDir/usr/bin/pdn/` (beside `AppRun`→`paintrs`) |
| windows | `win-x64` / `win-arm64` | `<zip dir>/pdn/` (beside `paintrs.exe`) |
| macos | `osx-arm64` | `paint.rs.app/Contents/MacOS/pdn/` (beside `paintrs`) |
| alpine (musl) | `linux-musl-x64` / `-arm64` | `<tarball root>/pdn/` |

This is **wired into `.github/workflows/release.yml`**: the linux, windows and macos jobs
set up .NET 11 (`actions/setup-dotnet`) and publish the self-contained host into `pdn/`
beside the binary (the linux tarball's `$DIST/pdn` is tarred in and copied into the AppDir;
the windows job publishes into `$dir/pdn`; macOS into the bundle's `Contents/MacOS/pdn`).
The `linux-x64` and cross-compiled `win-x64` publishes were verified locally (each yields a
complete host: `libhostfxr.so`/`hostfxr.dll` + coreclr + `PdnBridge.dll` + the replacement
assemblies). paint.rs starts a `pdn/` that carries its own hostfxr from there, as its app
host would (`hostfxr_initialize_for_dotnet_command_line`: hostfxr refuses runtimeconfig
initialisation for a self-contained app), and never looks for an installed .NET.

On Linux and macOS the host also carries opend2d's `libd2d1`/`libdwrite`/`libwindowscodecs` and GDI+.
The jobs build opend2d (github.com/daijro/opend2d) from a pinned source snapshot
(`scripts/ci/fetch-opend2d.sh`, SHA-256 checked) attached to an OpenD2D release;
`scripts/ci/pack-opend2d.sh <opend2d repo> <commit> <dir>` makes the next one and prints the pin.
Windows ships neither: the host there uses the system's Direct2D, DirectWrite and GDI+. Every publish
writes `pdn/notices/` (`bridge/collect-notices.py`): the .NET runtime, the NuGet packages
(ComputeSharp.D2D1's MIT text from its nuspec), the third-party crates linked into opend2d's drop-ins,
GDI+, and on macOS the Homebrew libraries bundled with libgdiplus; a release publish fails on a
missing one. Still to wire: **alpine/musl** (needs .NET in the Alpine build container, RID
`linux-musl-x64`) and **FreeBSD** (no Microsoft runtime build — stays framework-dependent
with a `dotnet` prerequisite, or omits the host until a community net10 build is pinned).

## Verification

- **Effect path:** `paintnet/tests/run.py selftest` renders an in-tree `Effect` through
  the host + the replacement set and checks the inverted pixels — the self-contained
  regression that proves the pipeline. Byte-identity against the *genuine* assemblies
  uses `run.py vm` + a third-party plugin and a Windows VM (needs opend2d's `ref/` and
  the VM; not run here).
- **FileType path:** `paintnet-filetypes` drives a real bundled codec's save/load; it
  needs the Linux native codec `.so` (rebuilt from the codec's open-source wrapper) and
  a bundled plugin DLL from opend2d's `ref/` (test-only).

## Third-party plugins & GDI+ (0.8.0, in progress — HANDOFF STATE)

Test corpus: **BoltBait Plugin Pack 6.11** (38 effects, net7) under the gitignored
`ref/pdn-plugins/` (see its `SOURCES.md`; extracted from the pack's managed installer with
`tools/pdn-plugin-extract`, classified by `ref/pdn-plugins/_dl/classify.py`). pdn-bridge is
byte-identical to pdnhost, so pdnhost stands in for the classification.

**Done:** widening the replacement set (the `PaintDotNet.Effects`/`PaintDotNet.Base` type-forwarder
assemblies, legacy `EffectFlags` ctors, `SubmenuNames`, `EffectEnvironmentParameters`,
`PropertyBasedGpuDrawingEffect`, `Vector3Double`/`Vector4Float`, `UIScaleFactor`, `ColorBgra.FromBgr`,
`ReflectionTypeLoadException` tolerance) takes the pack **0 → 38 loading, 16 classic effects
enumerated**.

**Render blocker = GDI+.** Most plugins touch `System.Drawing` (icons / drawing). Fix route
(from the 0.8.0 coordinator):
1. `libgdiplus` is **built from source, no sudo** — `mono/libgdiplus` 6.1 (commit `44b44f92`),
   `./autogen.sh && ./configure --without-gif && make -C src` (skip the bundled gtest, which fails
   on modern CMake). Output staged at `.build-tmp/gdiplus/install/lib/libgdiplus.so.0.0.0`. Ship it
   in `pdn/` as `libgdiplus.so`.
2. `System.Drawing.Common` must be the **6.0** unix build (last with the libgdiplus P/Invoke; set
   `AppContext` switch `System.Drawing.EnableUnixSupport=true` — done in `pdnhost` `Main`). The 6.0
   nupkg's `runtimes/unix/lib/net6.0/System.Drawing.Common.dll` goes in the asmdir.
3. The single pdn-host ALC resolves `System.Drawing.Common` by name to that 6.0 file for every
   requested version, so the compile-time 10.0 reference is irrelevant.

**ROOT CAUSE (SOLVED).** The ctor `MissingMethod` was **not** an ALC version dup. The BoltBait
pack is classic **.NET Framework 4.x** plugins (they reference `mscorlib 4.0.0.0`,
`PaintDotNet.Core 4.210`, and bind `System.Drawing.Image` from the old FX `System.Drawing`
assembly, PKT `b03f5f7f`, *not* `System.Drawing.Common`). Because `System.Drawing.dll` is a
shared-framework TPA assembly, the plugin-load-context's `Load` returned null for it, the
reference escaped to the **default** ALC, and that forwarded `Image` to a *second*
`System.Drawing.Common` (10.0) loaded there — while the replacement `Effect`'s `Image` was the 6.0
copy in the custom ALC. Two `Image` types ⇒ MissingMethod (an interactive `typeof(Image).Assembly`
probe on both sides confirmed this).

**Fix:** stage the framework's `System.Drawing.dll` **type-forwarder facade** into the asmdir
(`tests/stage-gdiplus.sh`, wired into `run.py build`, alongside the 6.0 `System.Drawing.Common` and
`libgdiplus.so`). The custom ALC now resolves `System.Drawing` itself; its forwards keep
`Image`/`Bitmap`/… on the one 6.0 `System.Drawing.Common` and `Color`/`Rectangle` on the framework
`System.Drawing.Primitives`. The host also drives the **6-arg classic `Render(token,dst,src,rois,
i,n)`** (not the modern 3-arg form) and populates `EnvironmentParameters`. `RenderArgs.Graphics`
now aliases the surface via a GDI+ `Bitmap` so draw-based effects run on `libgdiplus`.

**Renders now: 7 of the ~9 GDI+ effects** — Flip H, Flip V, HSVGradients, Polygon,
ComplementaryColors, GridCheckerboard, HueSatPlus. The last two were unblocked by giving
`ReadOnlyBoundToValueRule`2` the genuine `<TValue, TProperty>` generic order (read from
`ref/pdn-wine`), and by adding `UnaryPixelOp` + nested `UnaryPixelOps.HueSaturationLightness` +
`Surface.GetPointPointerUnchecked`. GDI+ **drawing** effects (Polygon/ComplementaryColors/
GridCheckerboard) draw through `RenderArgs.Graphics`; because libgdiplus does not write through a
bitmap-over-scan0, `RenderArgs` keeps a private bitmap, registers itself, and the host calls
`RenderArgs.FlushPending()` after each `Render` to copy it back. **Still blocked: SeamCarving ×2** —
they call into `System.Windows.Forms` at render time (no WinForms headless on Linux).

**Byte oracle — genuine Paint.NET 5.1.12 on the real Windows VM (real GDI+), via `tests/vm-oracle.sh`
(route D):** the VM's unserviced OS can't init net11 (the `ref/pdn-wine` build), but PDN 5.1.12
(net9) is installed, so pdnhost runs net9 framework-dependent on PDN's own runtime + assemblies,
read-only. Results (48×48): **5/7 byte-IDENTICAL — Flip H, Flip V (pure pixel), HSVGradients and
GridCheckerboard (axis-aligned GDI+ fills), and HueSatPlus** (after reimplementing `HsvColor` and
`UnaryPixelOps.HueSaturationLightness` with genuine's exact truncation / fixed-point arithmetic).
The two that differ are **anti-aliased GDI+ drawing** only — **Polygon** (edges, 745/9216 B, maxΔ=43)
and **ComplementaryColors** (text/swatch edges; its colours match, the residual is AA) — a known
libgdiplus-vs-Windows-GDI+ difference. (`pdnhost` is now dual-target: the single ALC lets the shared
framework resolve in the default context so `Rectangle` unifies, and `CreateEnvironment` builds
`EffectEnvironmentParameters` for the genuine ctor as well as the replacement's parameterless one.)

**Done — steps (3) and (4): the classic built-ins, `PaintDotNet.Data`, IndirectUI (Kaint comparison).**
The API surface was diffed against Kaint's `src/pdnbridge` (github.com/ssserch/Kaint, GPL-3.0,
carrying Paint.NET 3.36's MIT classic API) by scanning every TypeRef/MemberRef the corpus (BoltBait
pack, Mirror, PdnV5EffectSamples) makes against what the replacement set defines, base classes
included. Following Kaint, the built-ins are **managed** code in the replacement set, not the
function-pointer bridge to the Rust ports planned here before: that works unchanged in pdnhost,
pdn-bridge and the Mono web host, with no Rust ABI. Added:
- `PaintDotNet.Effects.Legacy`: GaussianBlurEffect, MedianEffect, SharpenEffect, EdgeDetectEffect,
  OilPaintingEffect, PencilSketchEffect and BrightnessAndContrastAdjustment, with genuine 5.1.12's
  names, `PropertyNames`, ranges and defaults; each body is the C# of pdn-core's genuine-exact
  `effect::bb` port (3.36's blur arithmetic differs from 5.x, so Kaint's bodies were not used).
  `assemblies/Effects/Forwarders.cs` forwards them for 5.0-era plugins.
- `PaintDotNet.Data`: `LayerBlendMode`, `LayerBlendModeUtil.CreateCompositionOp` (returns Core's
  `PaintDotNet.Rendering.CompositionOp`) and `UserBlendOps.*` — pdn-core's `pixel::ops::compose`
  for all 14 modes.
- Core: IndirectUI (`ControlInfo`, `PropertyControlInfo`, panels, tabs, the enums, adapted from
  Kaint's), `BinaryPixelOp`, `UnaryPixelOps.Desaturate`, the named `ColorBgra` colours and its
  `Color` conversion, `IEnumLocalizerFactory`/`IPalettesService`; Effects.Core: `Effect.Services`
  (enum labels + the default palette) and `OnCreateConfigUI`/`CreateDefaultConfigUI`/
  `OnCustomizeConfigUIWindowProperties` on the classic and GPU property effects; Base: the
  `PaintDotNet.Collections` helpers plugins build dropdowns with.
- `System.Windows.Forms` (`assemblies/WindowsForms`): a headless stand-in for what effects touch
  while rendering (SeamCarving reports through `Form.ActiveForm.Invoke(() => MessageBox.Show(…))`).
  pdnhost falls back to the asmdir for a System.* assembly the shared framework lacks.
- The bridge schema now carries IndirectUI labels (`docs/pdn-bridge.md`), so plugin dialogs show
  "Radius" rather than "Amount1".

*Results (pdnhost, BoltBait 6.11 + Mirror):* 44 DLLs load; classic effects that render went from
**8 to 16 of 17**. EdgeDetectClassic, SharpenClassic, GaussianBlurPlus, OilPaintingPlus, Pastel and
FloydSteinbergDithering are **byte-identical to pdn-core's genuine-exact ports** (64×48 mixed-alpha
image, defaults and non-default settings), and all 14 blend modes match `compose` over its colour
sweep. `crates/pdn-bridge/tests/builtins.rs` pins each built-in through the bridge. The GPU plugins
(24 effects in 21 DLLs) run through pdn-bridge, below.

**BBChart (Chart or Graph) renders through pdn-bridge.** It builds a WinForms `Chart` from its token's
XML and draws it with `Chart.SaveImage`. The bridge carries Microsoft's MIT chart control
(`assemblies/WindowsFormsDataVisualization`, vendored source, see its `PAINTRS-PATCH.md`) built against
the headless `System.Windows.Forms`, which now keeps a control's state and raises its property events
but has no window; `System.Resources.Extensions` (NuGet) reads the plugin's WinForms resources, and the
bridge runs plugin code under the plugin's load context as the contextual reflection context, so that
reader's `Bitmap` is the plugin's. BBChart's own dialog (`EffectConfigDialog`: a chart builder over
DataGridView, ToolStrip, SplitContainer, …) does not run, so it renders with its token type's defaults,
an empty chart, which it draws white (`PluginHost`: a classic effect whose plugin defines one
`EffectConfigToken` gets it default-constructed instead of null). Given the chart its dialog starts with
(a line chart of two series) as the token's XML, it draws that chart. The in-tree
`PdnSelfTest.ChartEffect` draws a column chart the same way (`crates/pdn-bridge/tests/chart.rs`).

**Paint.NET 5 GPU effects (pdn-bridge).** The replacement `PaintDotNet.Windows` / `.Windows.Core` /
`.Windows.Framework` / `.Effects.Core` / `.Effects.Gpu` assemblies carry Paint.NET 5's managed Direct2D
and DirectWrite API (device context, effects and their typed property accessors, brushes, geometry,
command lists, text) as thin wrappers over opend2d's `libd2d1` / `libdwrite` COM objects. Their vtable
calls are generated from opend2d's `crates/windows-shim/src/bindings.rs` by `paintnet/tools/gen_com.py`
(`assemblies/Windows/Interop/Com.g.cs`). Paint.NET's own custom effects (GaussianBlurEffect2, Mix,
InputSwizzle, the `Pdn*Effect`s) are composed from Direct2D built-ins. `bridge/GpuDriver.cs` drives the
GPU effect lifecycle (render info, environment, token, output) over one device context per render, at
Float32 buffer precision with the output read back as floats, and Paint.NET 5's CPU `BitmapEffect`
(output tile in the pixel format it picks; float formats are linear). Known colour spaces beyond
sRGB/scRGB (Adobe RGB, Display P3, ProPhoto, Rec.2020) are matrix/TRC ICC profiles built in
`Framework/Framework.cs`.
Plugins' pixel shaders use the genuine ComputeSharp.D2D1 3.2.0 (MIT, from NuGet), shipped beside the
bridge and shared from its load context. Paint.NET's own pixel shaders behind ScenePositionEffect,
HlslBinaryFunctionEffect, GaussianBlurMapEffect's per-pixel level selector and the sample-map renderer run
the same way, their bytecode copied from Paint.NET 5 into `WindowsFramework/PdnShaders.cs`. Against opend2d main 169d03a: BoltBait 6.11, 19 of the 21 GPU DLLs run
through paint.rs's app path; PdnV5EffectSamples, all 18 (four leave the test image unchanged at their
defaults, as genuine Paint.NET does). Black and White+ waits on the shader executor's `switch` (opend2d, the
thread's `pdn5-gpu-gaps.txt` G15); Text Fun Factory renders blank. BokehBlurEffect is Paint.NET's complex-kernel
bokeh (its kernels and arithmetic, on ConvolveMatrix/ArithmeticComposite), and BokehBlurMapEffect varies
its radius per pixel over it. The source is still uploaded at
8 bits: opend2d draws a Float32 bitmap by reading it as BGRA8 (G17).

*pdn-bridge on Linux (fixed):* the bridge shipped the Windows-only `System.Drawing.Common` 10.0,
so a plugin that loads its icon in its constructor (nearly every BoltBait plugin) failed there.
`bridge/publish.sh` now stages `System.Drawing.Common` 6.0 (unix) + `libgdiplus` beside the bridge
(`tests/stage-gdiplus.sh <dir>`; `EnableUnixSupport` is in its runtimeconfig), and the plugin load
context loads the runtime's `System.Drawing` facade itself so its forwards reach the bridge's copy.
Also fixed on that path: a managed null dereference killed the host process (Rust's 8 KiB
alternate signal stack is too small for CoreCLR's SIGSEGV handler; pdn-bridge now gives calling
threads 64 KiB), the effect environment (selection, source) is set as pdnhost sets it, GDI+
drawing is flushed back, and integer properties accept paint.rs's whole-valued doubles. The
paintrs test `boltbait_pack_loads_and_renders` installs the pack through the app's plugin manager
and renders all 15 classic BoltBait effects (byte-identical to pdnhost), and Chart or Graph (above).

*Tooling for the handoff.* `$TMPDIR/pdndump` (scratch, not committed) has `ilspycmd` under `.tools/`
and a `System.Reflection.MetadataLoadContext` dumper (`dump.cs`/`dump.csproj`, built to `out/`) that
lists a type's base chain, ctors, methods and fields — point both at `opend2d/ref/pdn-wine/*.dll`
(net11 genuine) with resolver dir = pdn-wine + the net10 runtime
(`/usr/share/dotnet/shared/Microsoft.NETCore.App/10.0.13`). Rebuild per the memory note
`pdn-host-step3-builtins`.

**Wine is not the oracle for GDI+ drawing** — Wine's GDI+ is its own reimplementation, so it would
agree with libgdiplus, not with Windows. Use the VM (route D above) for drawing effects; Wine only
confirms the pure-pixel path. When driving pdnhost headlessly over automation note two VM gotchas:
Windows Error Reporting swallows an unhandled exception's stderr (set `PDN_ERRLOG` to capture it to a
file), and running effects back-to-back in one session trips a transient `pdn1\work` access-denied on
load — run each in its own call after `Stop-Process pdnhost; Start-Sleep 2`.
