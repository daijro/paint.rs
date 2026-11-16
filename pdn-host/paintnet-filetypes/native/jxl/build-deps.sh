#!/bin/sh
# Builds the exact libjxl the bundled JpegXLFileType ships against — 0.11.2, the version
# the wrapper's vcpkg baseline pins — as static libs under .deps/ (gitignored), so
# libJpegXLFileTypeIO_X64.so links a pinned codec and does not depend on the system
# libjxl version (the system's 0.12 drives the streaming output differently). Run once
# before build.sh; needs network. Uses libjxl's bundled highway/brotli/skcms.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
deps="$here/.deps"
mkdir -p "$deps"
if [ ! -d "$deps/libjxl/.git" ]; then
    git clone --depth 1 -b v0.11.2 https://github.com/libjxl/libjxl.git "$deps/libjxl"
    git -C "$deps/libjxl" submodule update --init --depth 1 \
        third_party/highway third_party/brotli third_party/skcms
fi
cmake -G Ninja -S "$deps/libjxl" -B "$deps/libjxl/build-static" \
    -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DBUILD_TESTING=OFF \
    -DJPEGXL_ENABLE_TOOLS=OFF -DJPEGXL_ENABLE_BENCHMARK=OFF -DJPEGXL_ENABLE_EXAMPLES=OFF \
    -DJPEGXL_ENABLE_DOXYGEN=OFF -DJPEGXL_ENABLE_MANPAGES=OFF -DJPEGXL_ENABLE_JNI=OFF \
    -DJPEGXL_ENABLE_PLUGINS=OFF -DJPEGXL_ENABLE_SKCMS=ON -DJPEGXL_ENABLE_TRANSCODE_JPEG=OFF \
    -DJPEGXL_ENABLE_SJPEG=OFF -DJPEGXL_BUNDLE_LIBPNG=OFF \
    -DJPEGXL_FORCE_SYSTEM_BROTLI=OFF -DJPEGXL_FORCE_SYSTEM_HWY=OFF
ninja -C "$deps/libjxl/build-static" jxl jxl_cms jxl_threads
echo "built $deps/libjxl/build-static/lib/libjxl.a"
