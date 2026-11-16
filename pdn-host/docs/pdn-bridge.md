# pdn-bridge — the Paint.NET plugin host ABI

This is the contract between paint.rs (Rust) and the embedded .NET host that runs real
Paint.NET plugin DLLs. It is the protocol a *browser* host (.NET `browser-wasm` / Mono,
`pdn-host/paintnet-web`) must implement too — same JSON, same semantics, a different
transport (JS interop instead of a C ABI). Desktop and web share one managed host and one
set of replacement `PaintDotNet.*` assemblies (`pdn-host/`).

## Hosting model

The managed host `PdnBridge.dll` is ordinary IL (JIT), **not** NativeAOT: it must load
arbitrary plugin IL at run time, which NativeAOT cannot. paint.rs embeds CoreCLR and calls
into it:

1. `nethost` → `get_hostfxr_path()` locates `libhostfxr`.
2. `hostfxr_initialize_for_runtime_config(PdnBridge.runtimeconfig.json)` → a host context.
3. `hostfxr_get_runtime_delegate(ctx, hdt_load_assembly_and_get_function_pointer)`.
4. For each export below: `load_assembly_and_get_function_pointer(PdnBridge.dll, "PaintDotNet.Bridge.Exports", "<Method>", UNMANAGEDCALLERSONLY_METHOD, null, &fnptr)`.

Every export is a `static` method marked `[UnmanagedCallersOnly]` with a blittable
signature (pointers and 32-bit ints only). The Rust side binds them as
`extern "C"` function pointers. The browser host exposes the same methods as `[JSExport]`
instead; the JSON payloads are identical.

A loaded plugin lives in its own collectible `AssemblyLoadContext`: the plugin's private
managed deps resolve from the plugin's folder, the `PaintDotNet.*` identities and the BCL
resolve from the default context (the replacement set), and a Windows native codec helper
the plugin P/Invokes (`WebP_x64.dll`) is redirected to the matching Linux `.so`.

## Conventions

- **Strings** cross as NUL-terminated UTF-8 (`byte*`). Strings the host *returns* are
  allocated with `NativeMemory.Alloc`; the caller must release them with `PdnFree`.
- **Buffers** cross as `byte*` + an `int` length (or width/height). Pixel buffers the host
  returns are `PdnFree`-owned too.
- **Pixels** at the ABI are **BGRA8, straight (non-premultiplied) alpha, row-major, top
  row first, stride = width·4** — Paint.NET's classic `Surface`/`ColorBgra` layout.
  paint.rs stores layers as straight **RGBA8**, so the Rust binding swaps R↔B at the
  boundary; alpha is already straight on both sides (no premultiply).
- **Errors**: a function that can fail takes a trailing `byte** outError`. On success the
  host leaves `*outError = null` and returns 0 / a valid handle; on failure it returns a
  nonzero code / 0 handle and sets `*outError` to a `PdnFree`-owned UTF-8 message.
- **Handles** are opaque `void*` (a pinned `GCHandle` to managed state). 0 means none.
- **Cancellation**: the caller passes `int* cancel`; it stays 0, the caller sets it to 1
  from another thread to abort. The host polls it between render tiles; an aborted render
  returns a nonzero code and leaves the buffer unmodified.

## Exports

```c
// Load a plugin DLL and its dependencies into a fresh load context. Returns an opaque
// plugin handle, or 0 (outError set). (CoreCLR itself is started by the Rust host
// through hostfxr before any of these are called; there is no separate init export.)
void*    PdnLoadPlugin(const char* dllPath, char** outError);

// What the plugin provides, as JSON (see "enumerate" below). Caller PdnFree's the string.
char*    PdnEnumerate(void* plugin, char** outError);

// One effect's property schema, as JSON (see "effect schema"). effectId from enumerate.
char*    PdnEffectSchema(void* plugin, const char* effectId, char** outError);

// Render one effect in place over the BGRA8 buffer. props is the JSON object of property
// name -> value. (x,y,w,h) is the region of interest (= the selection bounds; selection
// masking/compositing is the caller's job). cancel may be null. Returns 0 on success.
int32_t  PdnEffectRender(void* plugin, const char* effectId, const char* propsJson,
                         uint8_t* bgra, int32_t width, int32_t height,
                         int32_t x, int32_t y, int32_t w, int32_t h,
                         int32_t* cancel, char** outError);

// Press the button `key` (a "button" property) of a classic effect's own settings window over the image
// (straight BGRA8), the window loaded as propsJson leaves it (its "$token", then the values). Returns
// {"token": "...", "properties": [...]}: the token the press made, to send back as "$token", and the
// window's settings after it, in the schema's form with each value as its default (a list's new items).
char*    PdnDialogPress(void* plugin, const char* effectId, const char* propsJson, const char* key,
                        const uint8_t* bgra, int32_t width, int32_t height, char** outError);

// The app's clipboard image (straight BGRA8, row-major) that plug-ins reading the clipboard see
// (Windows Forms' Clipboard, Paint.NET's clipboard service); bgra null empties it. One for every
// plug-in: set before rendering an effect whose enumerate entry has "clipboard": true.
int32_t  PdnSetClipboard(const uint8_t* bgra, int32_t width, int32_t height, char** outError);

// paint.rs's text and image codecs, for a System.Drawing without GDI+ (the browser host's,
// pdn-host/paintnet-web/drawing) to draw plug-in strings and read JPEG, TIFF and WebP with. paint.rs
// calls it once, after start. outline lays out one line (UTF-16 text, a UTF-8 family, an em in pixels,
// GDI+'s style bits) and returns floats (its width, height and ascent, then outline verbs) that
// freeFloats releases; decode returns an encoded image's width, height and straight BGRA8 as bytes
// freeBytes releases, or null. The desktop's own System.Drawing (libgdiplus) does not use them.
void     PdnSetDrawingHost(float* (*outline)(const uint16_t* text, int32_t len, const char* family,
                                             float px, int32_t style, int32_t* outLen),
                           void (*freeFloats)(float*, int32_t),
                           uint8_t* (*decode)(const uint8_t* bytes, int32_t len, int32_t* outLen),
                           void (*freeBytes)(uint8_t*, int32_t));

// FileType save: encode a BGRA8 image to the plugin's format. ftIndex from enumerate.
// Returns a PdnFree-owned byte buffer (length in *outLen), or null (outError set).
uint8_t* PdnFileTypeSave(void* plugin, int32_t ftIndex, const char* propsJson,
                         const uint8_t* bgra, int32_t width, int32_t height,
                         int32_t* outLen, char** outError);

// FileType open: decode bytes to a BGRA8 image. *outBgra is a PdnFree-owned width*height*4
// buffer; *outWidth/*outHeight the dimensions. propsJson carries the FileType's windows' answers
// ("FileType windows"), or is null. Returns 0 on success.
int32_t  PdnFileTypeOpen(void* plugin, int32_t ftIndex, const uint8_t* bytes, int32_t len,
                         const char* propsJson, uint8_t** outBgra, int32_t* outWidth,
                         int32_t* outHeight, char** outError);

void     PdnFreePlugin(void* plugin);   // unload the plugin's load context
void     PdnFree(void* ptr);            // release any buffer/string the host returned
```

## JSON: enumerate

`PdnEnumerate` returns what the plugin provides. `kind` distinguishes the effect family;
`gpu` effects are listed but `supported:false` until the GPU path lands (see seam below).

```json
{
  "effects": [
    {
      "id": "Mirror.MirrorEffect",
      "name": "Mirror",
      "submenu": "Distort",
      "adjustment": false,
      "configurable": true,
      "kind": "propertyBased",
      "clipboard": false,
      "supported": true
    }
  ],
  "fileTypes": [
    { "name": "WebP", "loadExtensions": [".webp"], "saveExtensions": [".webp"],
      "properties": [ { "name": "Quality", "type": "int", "default": 95, "min": 0, "max": 100 } ] }
  ],
  "errors": [ "Curves+: it needs its own settings window (a Windows Forms dialog), which paint.rs cannot show yet" ]
}
```

- `id` — the plugin's full .NET type name; stable, used as the paint.rs effect id (`find`,
  the menu action `menu.effects.<id>`, the remembered token).
- `name`, `submenu` — the effect's display name and `SubMenuName` (null → Effects ▸ Plugins
  root). `adjustment: true` places it under Adjustments.
- `configurable` — true when the effect exposes a non-empty `PropertyCollection`, or has its own
  settings window (below).
- `clipboard` — true when the plug-in refers to a clipboard (Windows Forms' `Clipboard` or Paint.NET's
  clipboard service): paint.rs then reads its clipboard image with each invocation and sets it
  (`PdnSetClipboard`) before rendering.
- `kind` — `classic` (`Effect`/`Effect<T>`), `propertyBased` (`PropertyBasedEffect`),
  `bitmap` (modern CPU), or `gpu`. The first two render through `PdnEffectRender` today.
- `supported` — false for families this host cannot run yet (`gpu`, `bitmap`); the UI
  lists them disabled.
- `fileTypes` — Paint.NET 5.2's file types and the classic ones (`PaintDotNet.FileType`,
  `PropertyBasedFileType`); `ftIndex` is the position here. `properties` is the type's save
  options, described as an effect's properties are (below), with their IndirectUI labels; absent
  when saving has no options.
- `errors` — what did not load, as the user reads it ("Name: reason"): each effect that could
  not be built (a missing API member, a constructor that throws, a WinForms settings window), and,
  when nothing else came of the DLL, why its types did not load. A DLL with no effects, no file
  types and no errors is a helper library.

## JSON: effect schema

`PdnEffectSchema` reflects the effect's `PropertyCollection` (and, where the replacement
assemblies carry it, the IndirectUI `ControlInfo`). Each property becomes one control. The
shape is deliberately close to paint.rs's declared-settings model so the existing
`effect_config` dialog builder renders it with no translation layer.

```json
{
  "id": "Mirror.MirrorEffect",
  "name": "Mirror",
  "properties": [
    { "name": "Angle",   "type": "double", "default": 0.0, "min": -180.0, "max": 180.0 },
    { "name": "Size",    "type": "int",    "default": 8,   "min": 1,   "max": 100 },
    { "name": "Smooth",  "type": "bool",   "default": true },
    { "name": "Quality", "type": "choice", "default": 1,
      "choices": [ { "value": 0, "label": "Low" }, { "value": 1, "label": "High" } ] },
    { "name": "Caption", "type": "string", "default": "", "maxLength": 255 }
  ]
}
```

Property type → paint.rs `Control`:

| schema `type` | source property | paint.rs `Control` |
| --- | --- | --- |
| `int`    | `Int32Property` (min, max)                 | `Range` (integer slider) |
| `double` | `DoubleProperty` (min, max)                | `Range` (float slider, step) |
| `bool`   | `BooleanProperty`                          | `Checkbox` |
| `choice` | `StaticListChoiceProperty` (valueChoices)  | `Select` (value/label pairs) |
| `string` | `StringProperty` (maxLength)               | `Text` |
| `color`  | `Int32Property` shown as an IndirectUI colour wheel; a dialog's colour swatch | `Color` (default `0xRRGGBB`) |

Labels come from the effect's IndirectUI description (`OnCreateConfigUI`), when it builds one:
a property's optional `label` is its control's `DisplayName` and `description` its `Description`
(a checkbox's caption), and a choice's `label` is its `SetValueDisplayName` name, else the value's
`ToString()`. paint.rs labels a checkbox with `description`, then `label`; any other control with
`label`, then `description`; and falls back to the property `name`. An effect whose
`OnCreateConfigUI` throws keeps the names.

An effect with its own Windows Forms settings window (a classic `EffectConfigDialog`, or a Paint.NET 5
`EffectConfigForm`) has no property collection: its schema lists the window's settings instead
(`bridge/DialogSettings.cs`). The host builds and loads the window headless and walks its controls. Each
value control is one property, named `w:` plus its place in the window, typed as above:
- track bars and numeric boxes;
- check boxes;
- radio groups (one choice);
- drop-downs, without their "..." command items;
- lists (`"type": "list"`): the selected index as the value (-1 for none), and the items as they are now
  (`"items"`, each its text, or for an item that only shows its type's name, its simple properties);
- buttons (`"type": "button"`, no value), other than the window's OK, Cancel and help ones and those that open
  a file chooser;
- text boxes;
- tool-strip drop-downs and toggles;
- controls whose click opens a colour dialog;
- a plug-in's own controls with a numeric `Value` between a minimum and a maximum, or a `Color`;
- a plug-in's curve editor (`"type": "curve"`, pyrochild's Curves+): a control with `SortedList<int, int>[]
  ControlPoints`, `int Entries` and `GetChannelName`. Its value is the points, a list per channel of `[x, y]`
  from 0 to `"max"`; `"channels"` and `"colors"` name and colour the channels. When a choice before it swaps
  the curve for one with other channels (Curves+'s mode), `"by"` names that choice and `"sets"` gives each of
  its values' `"channels"`, `"colors"` and `"default"` points. The curve is keyed by the panel holding it and
  replayed into the one that panel shows: its points are set and the control's value-changed event raised.
  Check boxes named for its channels (which ones a drag edits) are not settings; paint.rs's editor has its
  own.
- a window's displacement grid (`"type": "grid"`, `"w:grid"`, pyrochild's Grid Warp): a field of the window
  whose type has `SetPoint(x, y, dx, dy)`, `GetPoint(x, y)` (a vector with `X` and `Y`) and an `int Width` and
  `Height` in cells. Its nodes are (`"columns"` + 1) × (`"rows"` + 1), node `x` sitting at `(x + dx) ·
  (image width − 1) / columns` pixels. Its value is the nodes moved, `[x, y, dx, dy]` each in cells
  (`"default"`: none). `"columnsBy"` and `"rowsBy"` name the choices whose labels are its size. Replaying it
  moves the nodes in the grid the window then has, and waits for the window's own thread to bring its mesh up
  to date.

A control that moves with another (a track bar and the numeric box beside it) appears once, as the numeric
box. A label is the caption beside or above the control, else its group's, else its tooltip, else its name.
A render loads a fresh window and replays the values that differ from what the window shows, as a user's
input would arrive: drop-downs and radio groups first (one may be a preset that sets the others), then the
rest in order. A colour is replayed by answering the colour dialog the swatch opens. Then it takes the
window's token. A custom-drawn preview or canvas has no setting.

A button is never replayed: it is pressed when the user presses it (`PdnDialogPress`), as Paint.NET's
window edits the token it keeps (`bridge/TokenJson.cs`). The press returns the token, which the caller sends
back as `"$token"` with every later render and press. The window is loaded from it first, as Paint.NET hands a
dialog the token it kept, and then takes all the window's values, not only those that differ from the
schema's. The token is its fields as JSON. A token holding something JSON cannot carry (an image) stays in the
host for the session, and `"$token"` only names it (`"mem:<n>"`).

`PdnEffectRender`'s `propsJson` is the inverse: `{ "<name>": <value> }` where numbers are
numbers, booleans booleans, choices the chosen index/value, strings strings, colours `"#rrggbb"`
(or `"primary"`/`"secondary"`, the app's colours). Unknown or
out-of-range values are clamped by the property's own validation. `PdnFileTypeSave`'s takes the
save options the same way. A render's may also carry `"$environment"`: what Paint.NET tells an
effect about the app, `{ "primary": <BGRA word>, "secondary": <BGRA word>, "palette": [<BGRA
word>, …], "selection": [[x0, y0, x1, y1, …], …] }` (each colour a `ColorBgra`'s packed value);
without it the effect sees primary black, secondary white and the default palette. `selection`, sent when
only part of the image is selected, is the selection's outline: even-odd polygons in image pixels, the
ROI being their bounds. A classic effect gets the pixels it covers as its `PdnRegion`
(`EnvironmentParameters.GetSelection`). A GPU effect gets them as its selection's path geometry and
`RenderScans`, and the antialiased coverage as its `MaskImage`. A property type the
replacement `PaintDotNet.PropertySystem` does not yet implement is simply absent from the
schema; widening plugin coverage means adding that typed property to the replacement set.

## FileType windows

Some FileTypes show a window of their own while they open or save: TechnoRobbo's Icons, Animated Cursors
and PNG Sprite Matrix ask what to write, Midora's EMF and WMF what size to open at, over Paint.NET's main
window (a stand-in, `Application.OpenForms["MainForm"]`). paint.rs shows each as it shows an effect's own
window, and the open or save goes round again with the answers, on the desktop and in the browser alike
(.NET in the browser cannot wait for the user in the middle of a call):

- A window shown before paint.rs has answered it is read as an effect's own window is (the setting types
  under "JSON: effect schema", but no buttons) and cancelled. The FileType stops in its own way, and the
  open or save fails with `pdn-window:` followed by the window's JSON, `{ "title": "...", "properties":
  [...] }`, as the whole error text (also with `PDN_BRIDGE_TRACE`).
- The next run carries the answers in its props, `{ "$windows": [ { "<name>": <value>, ... }, ... ] }`,
  one per window in the order they show, each in `PdnEffectRender`'s value forms. The window shown k-th
  takes answer k: the settings given are set into it (choices first), then its OK button is clicked, so
  its handlers run, and it returns OK. An empty answer OKs the window as it opens.
- A save's answers go with that save only; paint.rs never remembers them as save options.

## GPU seam (deferred)

GPU effects (`GpuImageEffect`, ComputeSharp.D2D1 pixel shaders) reach Direct2D through the
`PaintDotNet.Direct2D1` wrappers, which bind to opend2d's portable `libd2d1`. That library
must be shipped beside paint.rs before GPU plugins run, so this ABI version lists them
(`kind:"gpu", supported:false`) and renders none. The future path adds one export —

```c
int32_t PdnEffectRenderGpu(void* plugin, const char* effectId, const char* propsJson,
                           uint8_t* bgra, int32_t width, int32_t height,
                           int32_t x, int32_t y, int32_t w, int32_t h,
                           int32_t* cancel, char** outError);
```

— backed by `pdn-host/paintnet-gpu`, with no change to the rest of the protocol.

## Verification

The host must be byte-for-byte faithful to Paint.NET. Two checks, both in `pdn-host/`:

- **Effects**: render a plugin through this host and through the upstream reflection host
  (`pdn-host/paintnet/host`, same replacement assemblies) over the same input and props;
  the BGRA output must be identical. Identity against the *genuine* Windows assemblies is
  the VM comparison in `pdn-host/paintnet/tests/run.py`.
- **FileTypes**: a save→open round-trip and a byte-compare of the encoded container against
  the upstream FileType host (`pdn-host/paintnet-filetypes`).
