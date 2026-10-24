# Web decode assets

Copied locally by `node scripts/copy-web-codecs.mjs <node_modules directory>` from an npm
install of the two packages below. The script validates the installed versions before copying;
it never installs packages or accesses the network. Runtime files are unmodified.

- `magick/`: @imagemagick/magick-wasm 0.0.44, its ES module and x86 `magick.wasm`
  (the package's `magick.wasm` export), Apache-2.0 LICENSE and complete delegate NOTICE.
- `jpegxr/`: jpegxr 0.3.0, its classic script with embedded wasm, package metadata and
  Microsoft BSD license files. No separate wasm file is required.

Trunk copies this directory, including licenses and NOTICE, to `assets/codecs/`.
The shim resolves it against the document base URL, including a deployed URL prefix.
Both fallback decoders initialize only when the browser cannot decode the format.
The JXR fallback retains paintdotjs's supported RGB sample formats. It explicitly refuses
planar alpha because npm jpegxr does not read the separate plane. Browser decoding is
still attempted first. Failed initialization remains an explicit shared rejection until the page is reloaded.

The Rust caller supplies the shared limits, 32768 per side and 512 MiB of RGBA on wasm.
The shim also bounds encoded bytes, total coded-item bytes, source sample bytes for JXR,
container depth to 8, and boxes/items/extents to 65536. HEIF item offsets must refer to the
local file or its item-data box. AV1 items need inline sequence headers; HEVC configurations
need a single established NAL length size and layer-0 sequence headers. Unsupported layouts
fail closed before a decoder runs. These admission limits do not promise a 512 MiB total
process-memory ceiling for browser engines or codec delegates and their working buffers.

`node tests/web-codecs.mjs [Trunk dist directory]` exercises real assets and fixtures in
Chromium beneath `/paintrs/`. This is a decoder harness. Task 57 must additionally
open `a.heic` in the actual app after Task 50 file flows exist and assert a 13 × 9 canvas.
