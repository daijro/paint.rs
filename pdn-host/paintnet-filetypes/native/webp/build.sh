#!/bin/sh
# Builds the Linux replacement for WebP_x64.dll from the vendored pdn-webp MIT
# wrapper, linked against the system libwebp (encode/decode/mux/demux). The output
# .so exports GetLibWebPVersion/WebPLoad/WebPSave with the same ABI the plugin
# P/Invokes, so the host can map "WebP_x64.dll" -> this file. Byte-identity with the
# Windows helper holds when the system libwebp release matches the one Windows bundled.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
out=${1:-"$here/libWebP_x64.so"}
cc -pipe -shared -fPIC -O2 -std=c++17 -fpermissive \
   -include "$here/linux_shim.h" \
   -I/usr/include/webp \
   "$here/upstream/WebP.cpp" "$here/upstream/WebPEncoder.cpp" "$here/upstream/WebPDecoder.cpp" \
   -lwebp -lwebpmux -lwebpdemux -lstdc++ \
   -o "$out"
echo "built $out"
