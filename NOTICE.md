# paint.rs attribution notice

paint.rs is an open source, clean-room reimplementation of Paint.NET, licensed under the GNU General
Public License version 3 or later (`LICENSE`, `LICENSE.md`). [paint.js](https://github.com/LabyStudio/paintdotjs)
by LabyStudio served as a reference during its development, and paint.rs includes code adapted from it
(not its artwork). paint.js is itself an unofficial JavaScript port of Paint.NET 3.36.7 whose interface targets
Paint.NET 5.1.12+.

The original Paint.NET source is Copyright (C) dotPDN LLC, Rick Brewster, and contributors.
Paint.NET was created by Rick Brewster. paint.js is Copyright (C) 2024-present LabyStudio.

paint.rs is not affiliated with or endorsed by dotPDN LLC, Paint.NET, or LabyStudio. The bundled
interface artwork (the icons, cursors and interface images in `assets/icons-svg/`) is original work
by paint.rs contributors, licensed under this project's GPL-3.0-or-later licence. paint.js's artwork
is not included.
No SVG geometry is derived from Microsoft Fluent icons; `assets/icons-svg/manifest.json`
records `fluentDerived: false`, so no Fluent icon notice applies. Bundled fonts in `assets/font/` keep their colocated `LICENSE.txt`;
they include Selawik (Microsoft, SIL Open Font License 1.1) and Carlito (tyPoland Łukasz
Dziedzic, SIL Open Font License 1.1).
Bundled UI fallbacks Noto Sans Arabic Version 2.013 (Copyright 2022 The Noto Project
Authors, https://github.com/notofonts/arabic) and Noto Sans Hebrew Version 3.001
(Copyright 2024 The Noto Project Authors, https://github.com/notofonts/hebrew) are
unmodified SIL Open Font License 1.1 faces. Their full licenses are colocated in
`assets/font/noto-sans-arabic/` and `assets/font/noto-sans-hebrew/`; pinned local package,
embedded copyright/version/license records, byte sizes and SHA-256 hashes are in
`assets/font/rtl-fallback-provenance.json`. They follow the existing interface faces
and provide Arabic/Hebrew glyph coverage without changing Latin font selection.
Additional document fonts in `assets/fonts/` are Noto Color Emoji Version 2.051,
Copyright 2022 Google Inc., from googlefonts/noto-emoji commit
`8998f5dd683424a73e2314a8c1f1e359c19e8742`, under SIL Open Font License 1.1
(`assets/fonts/noto-color-emoji/LICENSE`), and Anton Version 2.116,
Copyright 2020 The Anton Project Authors, from googlefonts/AntonFont commit
`beb92fcad87808357123bb66881b4032dc96efe7`, under SIL Open Font License 1.1
(`assets/fonts/anton/OFL.txt`). Pinned source URLs, sizes and SHA-256 hashes are in
`assets/fonts/provenance.json`. These fonts serve document content; they do not replace
bundled interface fonts. Color-glyph rendering is tracked separately by Task35c.
Script fallback fonts in `assets/fonts/noto-sans-*/` (R183 follow-up) are Noto Sans Devanagari,
Bengali, Gurmukhi, Gujarati, Oriya, Tamil, Telugu, Kannada, Malayalam, Sinhala and Thai
(Copyright 2022–2025 The Noto Project Authors, https://github.com/notofonts), unmodified from
the Arch Linux `noto-fonts 1:2026.10.01-1` package, and a subset of Noto Sans CJK SC Version
2.004 (© 2014-2021 Adobe, https://github.com/notofonts/noto-cjk) from `noto-fonts-cjk
20240730-1`, made by `scripts/fonts/subset_cjk.py` (common GB2312/JIS X 0208/KS X 1001
characters, Hangul and Kana; layout tables removed). All are under SIL Open Font License 1.1,
with each license colocated as `LICENSE.txt`; sources, sizes and SHA-256 hashes are in
`assets/fonts/script-fallback-provenance.json`.
paint.rs contains no GPC code. See LICENSE.md for the complete terms.

The application icon `assets/app-icon.svg` was supplied by the project owner (2026-10-24);
original file name `paint-rs-final.svg`. `assets/app-icon.png` is rendered
from it by paint.rs.

Image codecs: native builds link libjxl (the JPEG XL Project Authors, BSD-3-Clause) with its
bundled Highway (the Highway Project Authors, Apache-2.0 or BSD-3-Clause), Brotli (the Brotli
Authors, MIT) and skcms (Google Inc., BSD-3-Clause), and libwebp (Google Inc., BSD-3-Clause)
statically. The JPEG encoder (`jpeg-encoder`) is based in part on the work of the Independent JPEG
Group. Native builds decode AVIF with dav1d (Copyright © 2018-2025, VideoLAN and dav1d authors;
BSD-2-Clause), whose x86 assembly is built with x86inc.asm (the x264 project, ISC). The Linux and
macOS downloads link a pinned dav1d release statically and the Windows download carries the dav1d DLL.
HEIC and the AVIF container are read by heic-rs (MIT OR Apache-2.0,
https://github.com/tbraun96/heic-rs), vendored in `crates/vendor/heic-rs` with its licences and
NOTICE and modified by paint.rs (its `PAINTRS-PATCH.md`: the colour of files without an `nclx` box,
and the direction of `imir`). JPEG XR is decoded by openreadout-jpegxr, a port of Microsoft's jxrlib
decoder (Copyright (c) 2009, Microsoft; Copyright (c) 2026, The OpenReadout Authors; BSD-2-Clause,
the full text in that crate's LICENSE). The BSD licences above require their notices in the
documentation of binary distributions too (Task 58 packaging).

Web decoding uses @imagemagick/magick-wasm 0.0.44 (Dirk Lemstra, Apache-2.0), ImageMagick
and its delegates, and jpegxr 0.3.0 (Microsoft jxrlib, Brion Vibber; BSD-style license).
Their unchanged runtime assets, package metadata, LICENSE and complete delegate NOTICE are
bundled in `assets/codecs/` and copied into web distributions by Trunk. Regenerate them only
from the installed packages with `node scripts/copy-web-codecs.mjs`; see `assets/codecs/README.md`.

MobileSAM (Chaoning Zhang and the MobileSAM authors, Apache-2.0,
https://github.com/ChaoningZhang/MobileSAM) — Object Selection's segmentation model. Its weights are
`assets/models/mobilesam/mobile_sam-tiny-vitt.safetensors`: built into the desktop binary, and
served by the web build in two parts under `models/mobilesam/` with the licence, for its worker to
fetch when Object Selection's toolbar picks MobileSAM. They are a bit-identical safetensors
conversion of MobileSAM's official Apache-2.0 `mobile_sam.pt` by candle's author
(https://huggingface.co/lmz/candle-sam, revision `8b4cb7c7`; all 439 tensors match). MobileSAM's
Apache-2.0 license is `assets/models/mobilesam/LICENSE`, its `LICENSE` at commit `f706ad9`.
MobileSAM builds on Segment Anything (Meta Platforms, Inc., Apache-2.0) and TinyViT (Microsoft,
MIT). The model runs through candle (Hugging Face, MIT OR Apache-2.0).
`crates/pdn-core/src/extra/segment/tiny_vit.rs` and `mask_decoder.rs` are adapted from
candle-transformers 0.9.2 (Hugging Face, MIT OR Apache-2.0), used under MIT. That code ports
MobileSAM's `tiny_vit_sam.py`, which is TinyViT's architecture (Microsoft, MIT; it adapts LeViT,
Apache-2.0, and Swin Transformer, MIT), and Segment Anything's mask decoder (Apache-2.0).
candle's license (its `LICENSE-MIT` at commit `3b39794c`, which has no copyright line):

> Permission is hereby granted, free of charge, to any
> person obtaining a copy of this software and associated
> documentation files (the "Software"), to deal in the
> Software without restriction, including without
> limitation the rights to use, copy, modify, merge,
> publish, distribute, sublicense, and/or sell copies of
> the Software, and to permit persons to whom the Software
> is furnished to do so, subject to the following
> conditions:
>
> The above copyright notice and this permission notice
> shall be included in all copies or substantial portions
> of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
> ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
> TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
> PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
> SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
> CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
> OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
> IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
> DEALINGS IN THE SOFTWARE.

TinyViT's license (`TinyViT/LICENSE` in https://github.com/microsoft/Cream at commit `4a13c409`):

> MIT License
> Copyright (c) Microsoft Corporation.
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE

PhotoSauce.MagicScaler (Clinton Ingram, MIT License) — resampling kernels ported in
`crates/pdn-core/src/resample/magic.rs` (from version 0.15.0, https://github.com/saucecontrol/PhotoSauce).
MagicScaler is third-party code, not Paint.NET code. Its license:

> The MIT License
>
> Copyright (c) 2015-2024 Clinton Ingram and Contributors
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in
> all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
> THE SOFTWARE.

Some plugins were inspired by BoltBait's Paint.NET plugin pack. The `bb…` effects reproduce its
behaviour from a written specification; no BoltBait code or artwork is included.

pdn-content-aware-fill (Nicholas Hayes, GNU General Public License version 2 or later) — Content
Aware Fill, ported from commit `b54eec8b` (https://github.com/0xC0000054/pdn-content-aware-fill)
in `crates/pdn-core/src/extra/resynthesizer.rs`, `crates/pdn-core/src/effect/forum/content_aware_fill.rs`
and `crates/paintrs/src/dialogs/content_aware_fill.rs`, and used here under GPL-3.0-or-later as its
licence allows. Copyright (C) 2018, 2020, 2021, 2022, 2023, 2024, 2025 Nicholas Hayes. Its
Resynthesizer incorporates The Resynthesizer, a GIMP plug-in for resynthesizing textures (GPL-2.0
or later): Copyright (C) 2010 Lloyd Konneker, Copyright (C) 2000 2008 Paul Francis Harrison,
Copyright (C) 2002 Laurent Despeyroux, Copyright (C) 2002 David Rodríguez García. It is
third-party code, not Paint.NET code.

Basic Antialias and AA's Assistant (`crates/pdn-core/src/effect/forum/edge_aa.rs`) follow pyjo's
and dpy's Paint.NET plugins from their published behaviour; none of their code or artwork is
included. AA's Assistant's tap weights (dpy's published coefficients) and the form of its alpha
curve come from paint.c's clean-room AA's Assistant (https://github.com/vancevoj/paint.c,
`plugins/aa_assistant/`, at commit `d35776d9`); the code is written anew, and no paint.c code
is included. paint.c is third-party code, not Paint.NET code. Its license:

> MIT License
>
> Copyright (c) 2026 The paint.c authors
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.

Playground live editing: `crates/paintrs/src/live/playground_live.rs` is Playground's shared
live-editing template (github.com/daijro/openplayground, `patches/live/playground_live.rs`),
reformatted by rustfmt, with one test's time limit raised for slower test machines, and otherwise
unmodified.
It is by paint.rs's author, and Playground licenses it under each app's own licence, here
GPL-3.0-or-later (Playground's `patches/README.md`). `crates/paintrs/src/live.rs` and
`crates/paintrs/src/live/peers.rs` are adapted from Playground's live-editing patch for PhotoCraft
(`patches/photoshop/live.diff`), which Playground licenses under PhotoCraft's licence, below.

PhotoCraft (ArtCraft Team and the PhotoCraft contributors, MIT License) — algorithms for the Extra
effects and tools ported in `crates/pdn-core/src/extra/`, `crates/pdn-core/src/tool/` and
`crates/paintrs/src/dialogs/` (from commits `ff47388` and `e6d6fb45`), its PSD reader and writer and
its pen tablet readers, vendored in `crates/vendor/photocraft-psd/` and
`crates/vendor/photocraft-tablet/` (commit `e6d6fb45`, see their `PAINTRS-PATCH.md`), and the
live-editing adapter above. PhotoCraft is
dual-licensed MIT OR Apache-2.0; paint.rs uses it under MIT. It is third-party code, not Paint.NET
code. Its license:

> MIT License
>
> Copyright (c) 2026 ArtCraft Team and the PhotoCraft contributors
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.

PsdPlugin (Frank Blumenberg and Tao Yue, MIT License; https://github.com/PsdPlugin/PsdPlugin), the
Photoshop file type plugin for Paint.NET — `crates/pdn-core/src/io/psd.rs` follows how it opens
and saves PSD files: its blend mode mapping, its layer group marker names and its layer masks. Its
license:

> Copyright (c) 2006-2007 Frank Blumenberg
> Copyright (c) 2010-2020 Tao Yue
>
> MIT License: http://www.opensource.org/licenses/mit-license.php
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
> THE SOFTWARE.
