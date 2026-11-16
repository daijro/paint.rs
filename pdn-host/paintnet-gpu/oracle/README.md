# pdngpu — genuine Paint.NET v5 GPU-effect oracle (Windows / reference VM)

Headless host that drives a genuine Paint.NET v5 `GpuImageEffect` plugin to a BGRA bitmap through the
**genuine** PaintDotNet assemblies + native `d2d1`, producing the reference ("oracle") bytes the Linux
`paintnet-gpu` replacement path (over opend2d) is compared against. Pure reflection — references **no**
PaintDotNet assemblies — so it drives whatever assembly set it loads.

Why it is shaped this way (all load-bearing; see `~/.bb/thread-storage/pdnv5/findings.md` for the full trail):
- Genuine PDN 5.1.12 assemblies are **ReadyToRun composite win-x64** → `BadImageFormatException` on Linux,
  so the oracle is Windows-only (the reference VM, or 5.2 under Wine).
- PDN's effect/env/image interfaces are sealed (`IObjectRef.InternalImplementationOnly`), so the host does
  **not** implement them — it reflection-constructs PDN's own `EffectEnvironment` and drives the effect's
  own `GpuEffect.Renderer`.
- Run on PDN's **own** runtime (like `paintnet/tests/vm-oracle.sh` route D): stage PDN's install into a
  `pdnrun` dir (coreclr + genuine assemblies), drop `pdngpu.dll/.exe` + a self-contained runtimeconfig there,
  `--asmdir pdnrun`. Keep `pdnrun`'s `ComputeSharp.D2D1` = PDN's genuine R2R copy (`PaintDotNet.Core` is
  R2R-MVID-bound to it).
- The plugin loads in a custom ALC that takes PaintDotNet/BCL/TerraFX from the default context (so its
  `GpuImageEffect` base unifies with the host) but ComputeSharp from the **plugin dir** — the full nuget
  ComputeSharp (which has `Interop.D2D1PixelShaderEffect`/`D2D1DrawTransformMapper` that PDN's trimmed R2R
  copy lacks). The plugin only hands PDN `byte[]` blobs, so the two ComputeSharp instances never meet.

Usage: `pdngpu --plugin <dll> --asmdir <pdnrun> --effect <Type> --in <bgra> --out <bgra> --width W --height H [--props <json>]`.
Build/run on the VM is driven from the worktree by `ref/pdn-plugins/_gpuhost/{deploy,run}.sh` (gitignored;
they publish here, stage into `pdnrun`, and invoke per effect). The net9 sample plugin is built on the VM
against genuine 5.1.12 refs + ComputeSharp 3.1.0 (see `ref/pdn-plugins/SOURCES.md`).

Status: renders `HueRotateEffect` (and the other 5.1-compatible GPU samples) end-to-end. 5.2-only samples
(`SquareBlurGpu`, `ChannelBlur`, `SharpenGlitch`) need the queued 5.2/Wine oracle.
