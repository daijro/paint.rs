# hosts/paintnet — classic Paint.NET CPU plugins, hosted without Wine

Phase 5A.1. Runs official Paint.NET **classic** CPU effect plugins (`Effect`,
`PropertyBasedEffect`, IndirectUI, the classic surface types) unchanged on
Linux .NET, and proves the output is byte-identical to the same plugin running
against the genuine Paint.NET assemblies on Windows. Classic effects draw on
plain `Surface`/`ColorBgra` pixel buffers, not Direct2D, so this needs no
opend2d yet.

## How it works

A plugin is compiled against the *real* Paint.NET API. We provide that API two
ways with identical identities, namespaces, types and signatures:

* **`assemblies/`** — the ONE unified set of independent replacement assemblies
  (phase 5A-4), merging the former classic-effect shims (5A-1) and the FileType
  compat set (5A-3) into a single family: `PaintDotNet.Core`, `.Primitives`,
  `.Fundamentals`, `.ComponentModel`, `.PropertySystem`, `.Windows`,
  `.Windows.Core`, `.Effects.Core`, `.FileTypes`. Each builds one DLL with the
  real assembly identity, type placement and public signatures (read from the
  public metadata of the reference build); no decompiled implementation is
  copied, only the members the test corpus calls. They carry the reference
  version (`5.200.9793.3533`) so a plugin also binds, unchanged, to the genuine
  assemblies on Windows. The effect host, the FileType host
  (`hosts/paintnet-filetypes`) and the browser host (`hosts/paintnet-web`) all
  reference this one set.
* the genuine assemblies in `ref/pdn-wine`, used only as the Windows reference.

**`host/` (pdnhost)** is a pure-reflection host: it links none of the
PaintDotNet types, loads the plugin + its dependencies into an isolated
`AssemblyLoadContext`, finds the `Effect` subclass, builds its
`PropertyCollection`, sets property values, wraps the input image in a
`Surface`, calls `SetRenderInfo` + `Render`, and writes the result as raw BGRA.
Because the *same* binary drives both the replacement set (Linux) and the
genuine assemblies (Windows), any host-side scheduling (tiling, ROIs) is identical by
construction, so a byte difference can only come from the plugin's own pixels
or the API semantics — exactly what we want to test.

`tests/run.py` orchestrates: build → render locally → render on the VM against
the genuine assemblies → byte-compare.

```
hosts/paintnet/
  assemblies/                     the ONE unified replacement set (9 PaintDotNet.* projects)
    Core/                         ColorBgra, Surface, RenderArgs, EXIF/XMP, IPluginSupportInfo
    PropertySystem/               Property, typed properties, PropertyCollection, rules
    Effects.Core/                 Effect, Effect<T>, PropertyBasedEffect, tokens, options
    Primitives, Fundamentals, ComponentModel, Windows, Windows.Core, FileTypes
  host/                           pdnhost (reflection runner)
  tests/run.py                    build / local / vm / compare / all
  plugins/provenance.md           where each test plugin came from (binaries gitignored)
  dn.sh                           dotnet wrapper with sandbox-friendly cache dirs
```

## Build & run

```sh
cd hosts/paintnet
python3 tests/run.py build           # assemblies + host + plugins
python3 tests/run.py local           # -> artifacts/mirror.local.bgra
python3 tests/run.py vm               # genuine assemblies on the VM -> artifacts/mirror.vm.bgra
python3 tests/run.py compare          # must print IDENTICAL
```

`dn.sh` wraps `dotnet`, redirecting every cache (NuGet, CLI home, MSBuild temp)
into the gitignored `.build/` because the dev sandbox makes `~/.dotnet`,
`~/.nuget` and `/run/user` read-only; builds must also pass
`--disable-build-servers -nodeReuse:false` (an out-of-process MSBuild node
cannot complete its handshake under the sandbox).

### Target framework

Official Paint.NET 5.2 plugins target **net11.0**; this tree should too, so
genuine net11 plugin binaries load unchanged and the VM reference runs the
genuine net11 assemblies on a matching runtime. Install the SDK with:

```sh
curl -fsSL https://dot.net/v1/dotnet-install.sh | bash -s -- --channel 11.0 --install-dir hosts/paintnet/.build/dotnet
```

then point `dn.sh` at `.build/dotnet/dotnet` and bump the `<TargetFramework>`
in `assemblies/Directory.Build.props`, `host/pdnhost.csproj` and each plugin
csproj to `net11.0`. Until that SDK is present the tree targets `net10.0` (the
only ref pack installed), which builds and runs identically for these classic
types (the four bundled FileType plugins also target net10, except DDS).
In this environment the dotnet-install script and any freshly-installed
`dotnet` are blocked by the sandbox's "Code from External" execution gate, so
the net11 switch and all plugin execution need that permission granted first.

## Status

* The unified replacement set (9 assemblies) builds; the in-tree Invert
  `selftest` renders byte-exactly through `pdnhost` + the set, and the real
  bundled `WebPFileType.dll` binds to the same set and saves a valid WebP
  (see `hosts/paintnet-filetypes`).
* benbuck/MirrorEffect's unchanged source compiles against the set and its
  `Mirror.dll` binds exactly `PaintDotNet.{Core,Effects.Core,PropertySystem}` +
  `System.Drawing.*`.
* `pdnhost` builds and loads the plugin by reflection.
* Byte-identical verification of third-party effect plugins on the VM needs the
  sandbox's "Code from External" gate; see the task report.

Downstream (`hosts/paintnet-web`, `hosts/paintnet-filetypes`) reference the
`assemblies/` projects — one identity, shared by all three hosts.
