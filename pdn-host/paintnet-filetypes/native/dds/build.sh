#!/bin/sh
# Builds the Linux replacement for DdsFileTypePlusIO_x64.dll from the vendored
# pdn-ddsfiletype-plus wrapper (upstream/, v1.12.14) linked against the CPU DDS subset
# of DirectXTex (built by build-deps.sh). Exports CreateScratchImage /
# DestroyScratchImage / GetScratchImageData / Load / Save with the ABI the managed
# DdsNative P/Invokes, so the host maps "DdsFileTypePlusIO_x64.dll" -> this .so.
#
# The GPU BC6H/BC7 path is compiled but inert (DirectComputeHelper reports no device;
# gpu_stub.cpp satisfies the one GPU CompressEx symbol the wrapper names), so every
# format is encoded on the CPU.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
out=${1:-"$here/libDdsFileTypePlusIO_x64.so"}
deps="$here/.deps"
[ -f "$deps/libdirectxtex.a" ] || { echo "run build-deps.sh first" >&2; exit 1; }

inc="-I$deps/DirectXMath/Inc -I$deps/DirectX-Headers/include -I$deps/DirectX-Headers/include/directx -I$deps/DirectX-Headers/include/wsl/stubs"
c++ -shared -fPIC -O2 -std=c++17 -fvisibility=default -fopenmp \
    -include "$here/linux_shim.h" $inc \
    -I"$here/shim" -I"$here/upstream/DirectXTex" -I"$here/upstream" \
    "$here/upstream/DdsFileTypePlusIO.cpp" "$here/shim/gpu_stub.cpp" \
    "$deps/libdirectxtex.a" \
    -lstdc++ -lm -lpthread \
    -o "$out"
echo "built $out"
