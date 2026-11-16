# System.Drawing for the browser host

Plug-ins call System.Drawing (GDI+) to build icons, draw shapes and text into bitmaps, and read images. The
browser has no GDI+, and the System.Drawing.Common package throws on every GDI+ call off Windows. This project
is a System.Drawing.Common of our own for `app/PaintNetWeb.csproj`: the same assembly name, version and public
API as the 10.0 package, with the parts plug-ins use implemented in managed code.

The desktop bridge does not use it. It keeps System.Drawing.Common 6.0's Unix build over native libgdiplus
(`scripts/ci/build-gdiplus.sh`). There was no managed implementation there to share.

```
gen/            writes src/Api.g.cs from the package's public API (reflection over its metadata): every type and
                member, each body a stub that throws PlatformNotSupportedException naming the member
implemented.txt the members src/*.cs define, which gen leaves out of Api.g.cs
src/            the hand-written parts (below)
check/          draws shapes, text and images, then checks the pixels: dotnet run -c Release --project check
```

What is implemented:

- **Images** (`Images.cs`, `Icons.cs`, `Codecs.cs`, `Formats.cs`): Bitmap and Icon in straight BGRA, LockBits,
  Get/SetPixel, the serialization resources use, and PNG, BMP, GIF and ICO reading. PNG and BMP writing are here too.
  JPEG, TIFF and WebP are read with paint.rs's own codecs (below).
- **Graphics** (`Graphics.cs`, `Raster.cs`, `Geometry.cs`, `Regions.cs`, `Paint.cs`, `Named.cs`): every shape is
  flattened to polygons in device space. That covers the world transform, page unit and scale, GDI+'s pixel offset,
  the clip, antialiasing and source-copy compositing. Solid, linear-gradient, path-gradient, hatch and texture
  brushes are here, with pens (widths, dashes, joins, caps), GraphicsPath, Matrix, Region and DrawImage.
- **Text** (`Fonts.cs`, Graphics' Text, `Typesetter`): Font, FontFamily and StringFormat hold the settings.
  DrawString, MeasureString and GraphicsPath.AddString lay the text out here: wrapping, alignment, GDI+'s
  padding of a sixth of an em, the layout clip, underline and strikeout. paint.rs's Text outlines the glyphs with
  its bundled faces. Windows families paint.rs does not bundle map to the nearest bundled one.
- **Image attributes** (`Attributes.cs`): ColorMatrix, and DrawImage with an ImageAttributes' colour matrix.

paint.rs gives its text and image codecs as two app-context entries. `System.Drawing.TextOutline` is
`Func<string, string, float, int, float[]>` (pdn-core `effect::dotnet::text_outline`), and
`System.Drawing.DecodeImage` is `Func<byte[], byte[]>` (`effect::dotnet::decode_image`). In the browser,
`pdnBoot` passes them to the host as its `pdn` JS imports (`app/Host.cs`). On the desktop, the bridge's
`PdnSetDrawingHost` sets them, so the desktop stand-in below draws text as the browser does. Without the
entries, MeasureString estimates and DrawString throws.

Still stubs, because no plug-in in the packs reaches them: metafiles, HDCs and window handles, printing,
ImageAttributes' other adjustments, MeasureCharacterRanges, compressed BMPs, and writing anything but PNG and BMP.

## Rebuilding

```sh
dotnet run --project gen -- <System.Drawing.Common.dll 10.0> implemented.txt src/Api.g.cs
dotnet build -c Release                                           # bin/Release/net10.0/System.Drawing.Common.dll
dotnet run --project gen -- <System.Drawing.Common.dll 10.0> --missing bin/Release/net10.0/System.Drawing.Common.dll
dotnet run -c Release --project check
```

`--missing` lists every public member of the package the build lacks, and should print `0 missing`. The package's
DLL is in the NuGet cache (`pdn-host/bridge/.build/nuget/system.drawing.common/10.0.0/lib/net10.0`). To implement
a member, write it in a partial class under `src/`, add its key to `implemented.txt` (`gen -- <dll> --keys <Type>`
prints a type's keys), and regenerate.

## Which plug-ins run

The six packs (BoltBait, pyrochild, Ed Harvey, Red ochre, toe_head2001, MadJik), measured two ways on 2026-11-14.

**The desktop bridge with this assembly in place of libgdiplus.** This stands in for the browser runtime, with
paint.rs's text and codecs behind it, through `plugin_pack_triage`:

| System.Drawing                            | DLLs load | effects run | fail |
| ----------------------------------------- | --------- | ----------- | ---- |
| the 10.0 package (what the browser had)   | 1/205     | 12          | 1    |
| this assembly                             | 202/205   | 237         | 1    |
| desktop (System.Drawing 6.0 + libgdiplus) | 202/205   | 237         | 1    |

**Headless Chromium** (`../test/census.mjs` over the published AppBundle). Its page has no paint.rs, so it has no
text or JPEG decoding:

| System.Drawing                           | DLLs that list effects | effects listed | run | fail |
| ---------------------------------------- | ---------------------- | -------------- | --- | ---- |
| the 10.0 package                         | 0/207                  | 0              | 0   | 0    |
| this assembly                            | 173/207                | 175            | 161 | 14   |
| this assembly, with GPU and libraries in | 198/207                | 228            | 214 | 14   |

The last row adds GPU plug-ins (`../README.md`, "GPU effects in the browser") and the libraries plug-ins load
from the other installed DLLs.

None of the remaining failures is System.Drawing's:

- 9 effects draw text, which needs paint.rs on the page.
- 2 GPU effects draw text with DirectWrite, which finds no fonts in the wasm file system.
- 4 effects start threads (2 read the clipboard; pyrochild's Grid Warp and Smudge windows), and the browser
  runtime has none.
- Calendar's choices assume culture data, which the bundle leaves out.
- A few fail as they do on the desktop: Windows Forms and WPF types the shims lack, and a library a pack does not
  ship.
