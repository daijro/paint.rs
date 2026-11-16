#!/bin/sh
# Builds the Linux replacement for JpegXLFileTypeIO_x64.dll from the vendored pdn-jpegxl
# MIT wrapper (upstream/, commit f0a2bec), linked against libjxl (encode + decode +
# threads + cms). The output .so exports GetLibJxlVersion/LoadImage/SaveImage with the
# ABI the plugin P/Invokes, so the host maps "JpegXLFileTypeIO_X64.dll" -> this file.
# linux_shim.h supplies the CRT types/functions and neutralises the Windows SAL/DLL
# markers; shim/ holds the Windows SDK/HRESULT stand-ins the one Windows.h include needs.
# upstream/Decoder/JxlDecoderTypes.h carries a documented 1.3.0 ABI bridge (the bundled
# managed DLL is 1.3.0; the public wrapper source predates its setCicpColorInfo slot).
#
# Prefer the pinned static libjxl 0.11.2 from build-deps.sh — the wrapper's vcpkg-pinned
# version, whose streaming-output behaviour the wrapper matches (the system's 0.12 drives
# it differently) and the basis for byte-identity. Fall back to system libjxl if .deps/
# was not built.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
out=${1:-"$here/libJpegXLFileTypeIO_X64.so"}
d="$here/.deps/libjxl/build-static"
if [ -f "$d/lib/libjxl.a" ]; then
    jxlinc="-I$here/.deps/libjxl/lib/include -I$d/lib/include"
    jxllib="-Wl,--start-group $d/lib/libjxl.a $d/lib/libjxl_cms.a $d/lib/libjxl_threads.a \
            $d/third_party/brotli/libbrotlienc.a $d/third_party/brotli/libbrotlidec.a \
            $d/third_party/brotli/libbrotlicommon.a $d/third_party/highway/libhwy.a -Wl,--end-group"
else
    echo "note: .deps/libjxl not built (run build-deps.sh); linking system libjxl" >&2
    jxlinc="-I/usr/include"
    jxllib="-ljxl -ljxl_threads -ljxl_cms"
fi
cc -pipe -shared -fPIC -O2 -std=c++17 -fvisibility=default \
   -include "$here/linux_shim.h" -I"$here/shim" \
   -I"$here/upstream" -I"$here/upstream/Encoder" -I"$here/upstream/Decoder" $jxlinc \
   "$here"/upstream/*.cpp "$here"/upstream/Encoder/*.cpp "$here"/upstream/Decoder/*.cpp \
   $jxllib -lstdc++ -lm \
   -o "$out"
echo "built $out"
