#!/bin/sh
# Builds the exact libaom the bundled AvifFileType ships against (3.14.1), as a static
# lib under .deps/ (gitignored), so libAvifNative_x64.so links a pinned codec and does
# not depend on the system libaom version. Run once before build.sh; needs network.
# Byte-identity with the Windows AvifNative helper holds against this pinned libaom.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
deps="$here/.deps"
mkdir -p "$deps"
if [ ! -d "$deps/aom/.git" ]; then
    git clone --depth 1 -b v3.14.1 https://aomedia.googlesource.com/aom "$deps/aom"
fi
cmake -G Ninja -S "$deps/aom" -B "$deps/aom/build-static" \
    -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=0 \
    -DENABLE_TESTS=0 -DENABLE_TOOLS=0 -DENABLE_EXAMPLES=0 -DENABLE_DOCS=0 \
    -DCONFIG_AV1_HIGHBITDEPTH=1
ninja -C "$deps/aom/build-static" aom
echo "built $deps/aom/build-static/libaom.a"
