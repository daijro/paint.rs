# Paint.NET FileType plugins on Linux .NET (phase 5A-3)

Hosts Paint.NET **FileType** plugins (the new `PaintDotNet.FileTypes` API) off-Windows.
A plugin's managed IL is loaded unchanged; the Paint.NET identities it binds to are
supplied by independent replacement assemblies, and the Windows native codec helper it
P/Invokes (`WebP_x64.dll`, …) is replaced by a Linux `.so` rebuilt from the codec
wrapper's own open-source code.

## What works now (verified)

- The **official bundled `WebPFileType.dll`, `AvifFileType.dll` and `JpegXLFileType.dll`**
  (from `ref/pdn-wine/Bundled`, test-only, all net10) load in `host/`, their
  `IFileTypeFactory` is discovered, and their file types are enumerated — on Linux .NET 10,
  all against the one unified replacement set. (DDS is net11-only; no net11 runtime here.)
- A WebP **save** is driven end-to-end through the real plugin into the Linux WebP codec and
  emits a valid WebP. The embedded **VP8 image bitstream is byte-identical** to the bare
  encode from the native wrapper; only the metadata container differs.
- The WebP **load** path is wired: the real plugin decodes a WebP through the managed
  imaging layer (`ImagingFactory`/`CreateBitmap<T>`/`WriteSource`), builds a BGRA document
  with a bitmap layer and applies its XMP/EXIF metadata transactions. A lossless
  **save→load round-trip is byte-identical** (16×16, 1024 B).
- The **native wrapper is byte-identical to Windows**: `native/webp/` rebuilds pdn-webp's
  MIT C++ wrapper against system **libwebp 1.6.0**; for the same input + default options
  its output matches the official `WebP_x64.dll` byte-for-byte (SHA-256, cross-checked by
  running the official DLL under Wine). libwebp's encoder is deterministic per release, so
  the match holds wherever the system libwebp release equals the one Windows shipped.
- The **native wrapper is byte-identical to Windows**: `native/webp/` rebuilds pdn-webp's
  MIT C++ wrapper against system **libwebp 1.6.0**; for the same input + default options
  its output matches the official `WebP_x64.dll` byte-for-byte (SHA-256, cross-checked by
  running the official DLL under Wine). libwebp's encoder is deterministic per release, so
  the match holds wherever the system libwebp release equals the one Windows shipped.

## Layout

- replacement assemblies — this host references the ONE unified set in
  `../paintnet/assemblies/` (phase 5A-4): the FileTypes API, imaging interfaces, the
  typed property system, `ColorBgra32`/`RegionPtr`/`SizeInt32`, the EXIF/XMP model, and
  (shared with the effect host) `ColorBgra`/`Surface`/`Effect`. One identity now serves
  the effect host, this FileType host and the browser host. Fresh idiomatic C#; no
  Paint.NET code copied; scoped to the members the test corpus calls.
- `host/` — an isolating `AssemblyLoadContext` that resolves `PaintDotNet.*`/BCL from the
  default context, loads the plugin's private deps from its folder, and redirects the
  native helper (`<name>_x64.dll` → `lib<name>.so`); plus factory discovery and the
  save/load drive, and a flat-BGRA document.
- `native/webp/` — pinned pdn-webp MIT wrapper (`upstream/`, commit
  `ef4984a`), a Linux build shim and `build.sh`, and a `selftest.c` round-trip check.

## Build & run

```sh
cd hosts/paintnet-filetypes
sh native/webp/build.sh                              # -> native/webp/libWebP_x64.so
./dn.sh restore host/host.csproj                     # one online restore (nuget.config)
./dn.sh build   host/host.csproj -c Release --no-restore --disable-build-servers -nodeReuse:false
dotnet host/bin/Release/net10.0/pdnft-host.dll \
       <plugin.dll> native/webp save out.webp        # discover + save a test gradient
```
`dn.sh` keeps SDK/NuGet caches in `.build/` and disables the MSBuild node server (the dev
sandbox makes `~/.dotnet`/`~/.nuget` read-only and exposes `.gitmodules` as a device node).

## Native codec helpers (the off-Windows blocker)

All four bundled FileType plugins P/Invoke an x64 Windows helper DLL that cannot run on
Linux. Each helper is a thin open-source C++ wrapper around a portable codec library, so a
Linux `.so` rebuilt from that same source replaces it; byte-identity holds when the system
codec library's release matches the one Windows bundled.

| Plugin | Helper | Wraps | OSS wrapper (MIT) | System lib present | Status |
| --- | --- | --- | --- | --- | --- |
| WebPFileType (net10) | `WebP_x64.dll` | libwebp 1.6.0 | 0xC0000054/pdn-webp | libwebp.so.7 (1.6.0) | **rebuilt, byte-identical** |
| AvifFileType (net10) | `AvifNative_x64.dll` | libaom 3.14.1 | 0xC0000054/pdn-avif | pinned static (build-deps.sh) | **rebuilt, byte-identical** (decode via libaom, no dav1d) |
| JpegXLFileType (net10) | `JpegXLFileTypeIO_X64.dll` | libjxl 0.11.2 | 0xC0000054/pdn-jpegxl | pinned static (build-deps.sh) | **rebuilt, byte-identical** (one RE'd 1.3.0 encoder delta applied: rendering intent) |
| DDSFileTypePlus (**net11**) | `DdsFileTypePlusIO_x64.dll` | DirectXTex BC codecs | 0xC0000054/pdn-ddsfiletype-plus | — (DirectXTex is CPU-portable, MIT) | feasible with effort; also gated on net11 runtime (not installed) |

The AVIF and JPEG XL wrappers follow the WebP recipe (vendor the MIT wrapper, build against
the system lib); DDS additionally needs a Linux build of DirectXTex and a net11 runtime.
Their native ABIs are known (AVIF: `CompressColorImage`/`CompressAlphaImage`/`DecodeImage`/
`ReadColorImageData`/`ReadAlphaImageData`/`GetAOMVersionString`; JPEG XL: `LoadImage`/
`SaveImage`). Note a second gate beyond the `.so`: unlike WebP, the AVIF and JPEG XL **save**
paths drive a large colour-management managed surface (the `PaintDotNet.Imaging` CICP colour
primaries/space/transfer enums, `IColorContext` with CICP, `IFileTypePropertyBag`, HDR
metadata) that the unified set does not yet implement — so a native `.so` alone does not make
their save runnable. Both the wrapper port and that colour surface are needed per codec.

## Relationship to phase 5A-1 (`hosts/paintnet`) — merged (5A-4)

5A-1's effect shims and 5A-3's FileType compat set were binding-incompatible (two
different `PaintDotNet.PropertySystem.Property` constructors, two different
`PluginSupportInfoAttribute`). 5A-4 folded both into the ONE set in
`../paintnet/assemblies/` with the real signatures (the real `Property` base ctor is
`private protected`, so plugins only bind the typed subclasses — which now carry the
genuine overloads — and the real `PluginSupportInfoAttribute` is the effect host's
version). This host and the effect host now reference that single identity; the former
`compat/` set is gone.

## Remaining

- **Done:** the managed load path (document factory, bitmap layer/sink, metadata
  transactions, managed imaging layer) — WebP load round-trips byte-identically.
- **Whole-file byte-identity vs the official Paint.NET host** is still open, and gated on the
  *application's* auto-metadata, not the codec: the WebP VP8 bitstream and the native wrapper
  are already byte-identical to Windows, but Paint.NET's app attaches document metadata
  (resolution EXIF / sRGB colour context) that the plugin writes into the container. Unlike
  the effect host — which runs the *same* reflection binary against the genuine assemblies on
  the VM — this host *implements* the document/factory/sink interfaces, and the genuine
  versions carry a much larger surface (`IObjectRef`/`IInternalImpl`/the full metadata model),
  so it cannot be rerun against the genuine assemblies. Confirming byte-identity needs a
  capture of the real app's save on the VM (no headless app-save harness exists yet) or RE of
  `PaintDotNet.Data`'s Document→FileType metadata to reproduce the auto-metadata in the host
  document.
- AVIF / JPEG XL / DDS native wrappers: not built — see the table's second gate (wrapper port
  **and** the colour-management managed surface per codec); DDS also needs net11 + DirectXTex.
