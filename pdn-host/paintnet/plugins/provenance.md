# Test plugin provenance

Plugin binaries and third-party plugin source are **not committed** (see
`hosts/paintnet/.gitignore`); this file records where each came from and how to
reproduce it. Re-fetch with the URLs below, then build with
`tests/run.py build-plugin <name>` (or see each entry).

## mirror (benbuck/MirrorEffect)

- Kind: classic `PropertyBasedEffect` (CPU), single surface, geometric pixel copy.
- Source: https://github.com/benbuck/MirrorEffect — `MirrorEffect/Mirror.cs`
- Commit: `47a108f0d26f4052ffa13fe87fad446960fe210d`
- `Mirror.cs` sha256: `c7f24f2987f81493557f8cb409da5dc68c13535dbc291b1d62eb5a4eb88bea4b`
- License: none stated in the repo (README is "Copyright © 2020 Benbuck Nason").
  Used here only as an unmodified local test fixture; not redistributed, hence
  kept out of git. The original targets .NET Framework 4.7 and references the
  genuine PaintDotNet DLLs; we rebuild the unchanged source against our
  replacement assemblies (`plugins/mirror/Mirror.csproj`).
- Binds: `PaintDotNet.Core`, `PaintDotNet.Effects.Core`, `PaintDotNet.PropertySystem`,
  `System.Drawing.Common`, `System.Drawing.Primitives`.

## Candidates not yet wired up

- `mechaskrom/PaintNet-PS1Blend`, `PaintNet-DiffLayers` — Unlicense (public
  domain, committable), but modern layer-blend effects (need multi-layer input).
- BoltBait / pyrochild classic packs — behind a Cloudflare-gated forum
  (forums.paint.net returns 403 to automated fetches); grab manually and drop
  the DLL under `plugins/<name>/` to test.
