# hosts/paintnet-gpu — Paint.NET GPU plugins on opend2d (phases 5A-5 / 5A-6)

Paint.NET GPU effects (`GpuImageEffect` & friends, and ComputeSharp.D2D1 pixel shaders) reach
Direct2D through Paint.NET's `PaintDotNet.Direct2D1` / `PaintDotNet.Windows` wrappers. 5A-6 answered
**how those genuine wrappers reach `d2d1.dll`, and whether they bind to our portable `libd2d1`
unchanged** (merge 4be6b11: d2d-ffi now builds portable COM vtables off Windows), then folded the GPU
replacement into the single unified PaintDotNet assembly set.

## How the genuine wrappers reach d2d1.dll (read from ref/pdn-wine)

Paint.NET uses **PointerToolkit's TerraFX.Interop.Windows** + `PaintDotNet.Interop.ComObject`, which do
exactly two things (verified by decompiling the assemblies):

1. **Free functions are classic `[DllImport]`.** `TerraFX.Interop.DirectX.DirectX.D2D1CreateFactory`
   is `[DllImport("d2d1", ExactSpelling = true)]`. TerraFX also installs a `NativeLibrary`
   `DllImportResolver` with an app-settable `ResolveLibrary` hook.
2. **COM interface methods are raw function-pointer vtable calls:** every `ID2D1*` is a `struct` whose
   first field is `void** lpVtbl`, and each method is
   `((delegate* unmanaged[MemberFunction]<…>)lpVtbl[slot])(this, …)`. `ComObject` holds `T*` unmanaged
   pointers and calls `QueryInterface`/`AddRef`/`Release` the same way.

There is **zero `ComWrappers` / RCW / `Marshal` COM** anywhere in `PaintDotNet.Windows`,
`PaintDotNet.Windows.Core`, `PaintDotNet.Effects.Gpu` or `ComputeSharp.D2D1` (string-scanned: 0 hits).
So the whole path is platform-agnostic pointer math — the browser-`ComWrappers` worry in
notes/pdn-host.md simply does not apply to Paint.NET's actual code.

## Verdict: the interop model binds unchanged — only the API surface and the toolchain limit it

`binding/` **proves the mechanism binds to our portable `libd2d1` on Linux .NET**: it reproduces
TerraFX's IL pattern (a `[DllImport("d2d1")]` resolved by a `NativeLibrary` resolver + member-function
vtable calls) and drives `libd2d1` to create a factory, `QueryInterface` it (positive + negative), and
render+read back a premultiplied opaque-red fill — byte-exact, the same result `portable-test.c`
proves from C, now from managed code through the member-function calling convention:

```
D2D1CreateFactory (DllImport d2d1 -> libd2d1) hr=00000000 factory=True
QI(Factory1)=00000000  QI(random)=80004002
top-left BGRA = 0,0,255,255 (pitch 16)
GENUINE-MECHANISM OK
```

So the genuine wrappers **can** bind. Two things stop us running the *literal* genuine DLLs today, so
only part can — the smallest replacement stays:

- **The genuine assemblies are net11.** `TerraFX.Interop.Windows.dll` references
  `System.Private.CoreLib 11.0.0.0`; compiling/loading it needs the net11 runtime the owner defers
  (only net10 is installed). `binding/` therefore reproduces the pattern on net10 rather than
  referencing the net11 DLL.
- **`libd2d1`'s portable build exports only the render/read-back slice** (`portable.rs`:
  factory→device→context→bitmap→clear/solid-fill→map). A GPU effect needs `CreateEffect` /
  `ID2D1Effect` / `DrawImage` / `ID2D1EffectContext.LoadPixelShader` and `ID2D1DeviceContext6`
  drawing — not yet exported off Windows (owned by the d2d-ffi track). Until that lands, the GPU
  pixel-shader slice is served by the native bridge, not by the genuine wrappers over COM.

## What is replaced, folded into ONE PaintDotNet identity (5A-6)

The GPU wrapper surface used to be a separate made-up assembly `PaintDotNet.Direct2D1.Gpu`. It is now
folded into the single unified `hosts/paintnet/assemblies` set with the **genuine** assembly
identities, so there is one `PaintDotNet.Direct2D1` identity shared by every host, not two:

| namespace | genuine assembly | folded into |
| --- | --- | --- |
| `PaintDotNet.Direct2D1` (IDeviceImage, SourceImage, PixelShaderEffect, DeviceContext) | `PaintDotNet.Windows` | `assemblies/Windows/Direct2D1Gpu.cs` |
| `PaintDotNet.Effects.Gpu` (GpuImageEffect, IEffectEnvironment) | `PaintDotNet.Effects.Core` | `assemblies/Effects.Core/EffectsGpu.cs` |
| `ComputeSharp.D2D1` (ID2D1PixelShader, D2D1PixelShaderEffect) | `ComputeSharp.D2D1` (3.2.0) | `assemblies/ComputeSharp.D2D1/` |

`DeviceContext.DrawImage` is still the smallest replacement: a pixel-shader effect's DXBC + constant
buffer + input image are handed to the native bridge (`native/d2dgpu`), which runs the shader through
`crates/d2d-shader`'s WARP-exact SM4/5 executor (phase 3C.1). No Paint.NET code is copied; only the
members the in-tree test effect calls are present (scoped like the rest of the set).

```
plugin GpuImageEffect (precompiled DXBC embedded)
  → PaintDotNet.Direct2D1 / .Effects.Gpu / ComputeSharp.D2D1   (the ONE unified set)   managed, C#
    → P/Invoke  d2dgpu_render(dxbc, cbuffer, input, …)                                 native bridge
      → crates/d2d-shader  Shader::run  (WARP-exact SM4/5 executor)  == native d2d1.dll on WARP
```

## Byte-identical results (verified)

- **`ScaleSampleGpuEffect`** (a `GpuImageEffect` shipping a precompiled shader-06 pixel shader) through
  the **folded** surface == the committed WARP capture: `IDENTICAL: 64x64 f32 == WARP capture (65536
  bytes)`. A match against that capture is a match against native Direct2D on WARP (captured by
  `harness/shader-ref` on the VM in 3C.1).
- **Native bridge** `cargo test` vs the committed `shader-*` WARP captures: nearest & arith & flow
  byte-exact, linear ≤1 LSB alpha (3C.1's documented sampler residual).
- **`binding/`**: the genuine interop mechanism renders byte-exact through `libd2d1` (above).

## Built-in GPU effects end to end (`tests/builtin/`)

The built-in effects in `PaintDotNet.Effects.Gpu.dll` are ComputeSharp.D2D1 pixel shaders that read
Direct2D-supplied varyings: input effects sample `D2DGetInput(n)` (the default VS's `TEXCOORD`n,
1:1-mapped for `[D2DInputSimple]`), generators read `D2DGetScenePosition()` (`SCENE_POSITION`), and
some bind resource textures. The oracle-capture gap the old notes flagged is **already closed** by two
pieces now on `main`: the 2B-3 `register_shader_effect` scene op builds a **real `ID2D1Effect`**
(`ID2D1EffectImpl` + `ID2D1DrawTransform`, `SetPixelShader`/`SetInputDescription`) so native d2d1 runs
the DXBC on WARP with its own varying realization (the fxshader-01 oracle), and crates/d2d's
`effects/shader.rs` reproduces that realization on the CPU pull model. So a built-in shader runs through
**the same effect graph** `d2d-ref` captures from, not the SV_Position-only `native/d2dgpu` bridge.

`tests/builtin/extract.py` classifies all **50 ComputeSharp pixel-shader passes** (across 27 built-in
effects) from the gitignored `ref/pdn-wine` DLL and writes each one's DXBC into `.build/shaders/`
(never committed — it is Paint.NET bytecode). The classification (`effects.json`, committed — facts
only, no bytecode):

| what `effects/shader.rs` runs 1:1 | passes |
| --- | ---: |
| **runnable** (simple inputs only, no resource texture) | **19** |
| blocked — complex / neighbourhood sampling (`D2DSampleInputAtOffset/Position`): Mosaics, SampleMap resamplers, blurs, Pointillism, StainedGlass, Fragment, Linocut, Halftone Screen | 23 |
| blocked — resource texture (`Texture1D`/`2D` beyond the inputs): Clouds, AddNoise, OrderedDither, Crystalize, Dents, Spherize, ReduceNoise, SurfaceBlur, MosaicPackedCells | 8 |

Of the 19 runnable passes, 11 use `sin`/`cos` (compiled to the SM `sincos` opcode — which d2d-shader
runs with host `libm`, the documented 3C.2 residual, so not yet byte-exact to WARP); that leaves **8
no-trig passes** that execute byte-exact, of which 5 are single-input colour/transfer maps whose 8-bit
output is a meaningful comparison (the other three are SampleMap coordinate-map passes whose output
clamps to a near-uniform map at 8 bit — and whose full effect needs the complex resample stage anyway).

`tests/builtin/capture.py` captures the WARP oracle for those 5 (`pdngpu2-01..05`) via `d2d-ref`, and
`verify/` renders each through crates/d2d's effect graph and compares byte-for-byte. See
`## Built-in effect verification` below for the result.

## Built-in effect verification

```sh
cd hosts/paintnet-gpu
# 1. classify all 50 shader passes + extract the capture-set DXBC (needs ref/pdn-wine + its ilspycmd dump)
python3 tests/builtin/extract.py \
  ../../ref/pdn-wine/PaintDotNet.Effects.Gpu.dll <(ilspycmd ../../ref/pdn-wine/PaintDotNet.Effects.Gpu.dll) .build/shaders
# 2. capture the WARP oracle (reference VM, under flock /tmp/d2d-vm.lock, allowed_domains the VM host)
python3 tests/builtin/capture.py tests/builtin/effects.json .build/shaders .build/scenes ../../conformance/oracle/win11-26100
# 3. run each shader through crates/d2d's effect graph and compare byte-for-byte to the oracle
cargo run --manifest-path verify/Cargo.toml --release -- \
  tests/builtin/effects.json .build/shaders ../../conformance/oracle/win11-26100
```

`verify/` skips a shader whose DXBC (gitignored) or oracle capture is absent, so it is green in a clean
checkout. With both present it renders `HueSaturation`, `BrightnessContrast`, `FullContrast`,
`InkSketch Adjust` and `Halftone Separate` through `d2d::effects::PixelShaderEffect` and asserts the
32×32 premultiplied BGRA8 is byte-identical to native Direct2D on WARP:

```
PASS pdngpu2-01-huesaturation      (4096 bytes byte-identical to WARP)
PASS pdngpu2-02-brightnesscontrast (4096 bytes byte-identical to WARP)
PASS pdngpu2-03-fullcontrast       (4096 bytes byte-identical to WARP)
PASS pdngpu2-04-inksketch-adjust   (4096 bytes byte-identical to WARP)
PASS pdngpu2-05-halftone-separate  (4096 bytes byte-identical to WARP)
```

**5 of 5 byte-identical** — five actual Paint.NET built-in GPU effect shaders meet the fxshader-01 bar
(the captures `pdngpu2-01..05.{bgra,json}` are committed; each was captured twice on WARP and the two
runs were byte-identical). That is every runnable pass whose 8-bit output is a meaningful comparison:
the remaining 3 no-trig runnable passes are SampleMap coordinate maps (clamp near-uniform at 8 bit), and
the 11 trig passes wait on a WARP-exact `sincos` in d2d-shader (below).

## Build & run

```sh
cd hosts/paintnet-gpu
cargo test --manifest-path native/d2dgpu/Cargo.toml --offline   # native bridge vs WARP oracle
python3 tests/run.py all                                        # build, host vs WARP, binding proof
python3 tests/run.py binding                                    # genuine interop mechanism vs libd2d1
```

`dn.sh` keeps SDK/NuGet caches in `.build/` and disables the MSBuild node server (the dev sandbox
makes `~/.dotnet`/`~/.nuget` read-only and exposes `.gitmodules` as a device node). The host targets
`net10.0` (the installed runtime); official GPU plugins target `net11.0-windows`. One NuGet restore
needs the network (net10 ref/host packs + the shared set); later builds are offline. The `binding`
proof needs `libd2d1` (`cargo build -p d2d-ffi`, done by `tests/run.py build`).

## Remaining blockers

For the built-in GPU **shaders** (what `effects/shader.rs` can run byte-exact), in order of how many
passes they gate: (1) **`sincos`** — d2d-shader runs the SM `sincos` opcode with host `libm`, not WARP's
kernel (3C.2 stop-point), so the 11 trig-using runnable passes (generators, rotational SampleMaps,
Vignette) are not yet byte-exact; (2) **complex / neighbourhood sampling** (`effects/sample.rs`'s WARP
fixed-point sampler is not wired into the shader node), gating 23 passes; (3) **resource textures**
(no `Texture1D`/`2D` binding in the shader node), gating 8 passes.

For an unchanged official GPU **plugin** (beyond the shaders): (4) a portable rendering/effect C ABI in
`d2d-ffi` (this bridge is the pixel-shader slice); (5) multi-pass effect-graph composition + the
built-in effects' RGB↔HSL/CMYK sub-graphs (several effects chain two ComputeSharp passes plus D2D
built-ins); (6) the net11 runtime + genuine ComputeSharp.D2D1 COM-interop identity fidelity (the folded
set is still the scoped member subset, not a binding-compatible identity for an arbitrary plugin).
