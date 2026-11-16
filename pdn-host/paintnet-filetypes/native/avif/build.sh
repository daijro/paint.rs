#!/bin/sh
# Builds the Linux replacement for AvifNative_x64.dll from the vendored pdn-avif MIT
# wrapper (upstream/, commit 9412f6e), linked against the system libaom (encode+decode).
# The output .so exports CompressColorImage/CompressAlphaImage/DecodeImage/
# ReadColorImageData/ReadAlphaImageData/FreeDecoderImageHandle/GetAOMVersionString with
# the same ABI the plugin P/Invokes, so the host maps "AvifNative_x64.dll" -> this file.
# linux_shim.h supplies the CRT types/functions and neutralises the Windows SAL/DLL
# markers the Windows SDK headers would otherwise provide; shim/ holds empty stand-ins
# for the Windows SDK version headers TargetVer.h includes.
#
# Byte-identity with the Windows helper holds only when the system libaom release equals
# the one Windows bundled (3.14.1); libaom's AV1 encoder is deterministic per release.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
out=${1:-"$here/libAvifNative_x64.so"}
# Prefer the pinned static libaom 3.14.1 from build-deps.sh (byte-identity + no system
# dependency); fall back to the system libaom if .deps/ was not built.
if [ -f "$here/.deps/aom/build-static/libaom.a" ]; then
    aominc="-I$here/.deps/aom"
    aomlib="-Wl,--start-group $here/.deps/aom/build-static/libaom.a -Wl,--end-group -lpthread"
else
    echo "note: .deps/aom not built (run build-deps.sh); linking system libaom" >&2
    aominc="-I/usr/include"
    aomlib="-laom"
fi
cc -pipe -shared -fPIC -O2 -std=c++17 -fvisibility=default \
   -include "$here/linux_shim.h" -I"$here/shim" $aominc \
   "$here"/upstream/*.cpp \
   $aomlib -lstdc++ -lm \
   -o "$out"
echo "built $out"
