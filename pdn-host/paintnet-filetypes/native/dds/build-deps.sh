#!/bin/sh
# Fetches the two header-only Microsoft packages DirectXTex needs to build off-Windows
# (pinned), then compiles the CPU DDS subset of the vendored DirectXTex (upstream/,
# DIRECTX_TEX_VERSION 209) into a static lib. No system dependency; reproducible.
#
#   DirectX-Headers  v1.721.2  -> HRESULT/COM/GUID/DXGI_FORMAT + SAL stubs (wsl/)
#   DirectXMath      may2026   -> SIMD math types
#
# The GPU codec paths (DirectXTexCompressGPU/D3D11/D3D12), WIC and the other image
# containers are deliberately not built: the DDS saver runs the CPU encoder only.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
deps="$here/.deps"
mkdir -p "$deps"

DXMATH_TAG=may2026
DXHDR_TAG=v1.721.2

clone() { # url tag dir
    [ -d "$3/.git" ] && return 0
    git clone --quiet --depth 1 --branch "$2" "$1" "$3"
}
clone https://github.com/microsoft/DirectXMath     "$DXMATH_TAG" "$deps/DirectXMath"
clone https://github.com/microsoft/DirectX-Headers "$DXHDR_TAG"  "$deps/DirectX-Headers"

# The DirectX-Headers stub nests LARGE_INTEGER's LowPart/HighPart under a named `.u`,
# but DirectXTex's DDS callback-I/O (0xC0000054's addition) reads them Windows-style as
# direct members. Add the anonymous struct real Windows has. Idempotent, keyed on marker.
basetsd="$deps/DirectX-Headers/include/wsl/stubs/basetsd.h"
if ! grep -q 'DDSFILETYPEPLUS_ANON_LI' "$basetsd"; then
    python3 - "$basetsd" <<'PY'
import sys
p = sys.argv[1]; s = open(p).read()
anon = "{\n  struct { uint32_t LowPart; int32_t HighPart; }; // DDSFILETYPEPLUS_ANON_LI\n"
s = s.replace("typedef union _LARGE_INTEGER {", "typedef union _LARGE_INTEGER " + anon, 1)
s = s.replace("typedef union _ULARGE_INTEGER {",
              "typedef union _ULARGE_INTEGER {\n  struct { uint32_t LowPart; uint32_t HighPart; }; // DDSFILETYPEPLUS_ANON_LI\n", 1)
open(p, "w").write(s)
PY
fi

inc="-I$deps/DirectXMath/Inc -I$deps/DirectX-Headers/include -I$deps/DirectX-Headers/include/directx -I$deps/DirectX-Headers/include/wsl/stubs"
cxxflags="-O2 -std=c++17 -fPIC -fvisibility=hidden -DNDEBUG -fopenmp"
obj="$deps/dxtex-obj"; mkdir -p "$obj"
for f in DirectXTexImage DirectXTexUtil DirectXTexConvert DirectXTexCompress DirectXTexDDS DirectXTexMipmaps DirectXTexResize BC BC4BC5 BC6HBC7; do
    c++ $cxxflags -include "$here/linux_shim.h" $inc -I"$here/upstream/DirectXTex" \
        -I"$here/shim" -c "$here/upstream/DirectXTex/$f.cpp" -o "$obj/$f.o"
done
ar rcs "$deps/libdirectxtex.a" "$obj"/*.o
echo "built $deps/libdirectxtex.a"
