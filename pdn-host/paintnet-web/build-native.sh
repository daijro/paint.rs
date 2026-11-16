#!/usr/bin/env sh
# Build opend2d's Direct2D/DirectWrite drop-ins as wasm static archives for the browser host.
#
#   ./build-native.sh <opend2d-worktree>
#
# The replacement PaintDotNet.Direct2D1/DirectWrite wrappers P/Invoke D2D1CreateFactory ("d2d1") and
# DWriteCreateFactory ("dwrite") and reach everything else through COM vtables, as on the desktop.
# PaintNetWeb.csproj links the archives with <NativeFileReference> and the wasm publish relinks them
# into dotnet.native.wasm. Mono-wasm keys its P/Invoke table by archive file name, so they are d2d1.a
# and dwrite.a (not lib*.a). d2d-ffi links dwrite-ffi, so d2d1.a carries both factories; dwrite.a is an
# empty archive that only names the module (a second Rust archive would duplicate std).
#
# Prereqs: the .NET wasm-tools workload (for emscripten) under $DOTNET_ROOT (default ~/.dotnet),
# rustup's wasm32-unknown-emscripten std, and binaryen >= 118 on the pack's bin/ (README "GPU effects
# in the browser"). Build opend2d from a `git worktree add` of its main, never the main checkout.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
opend2d="${1:-${OPEND2D_DIR:-}}"
[ -n "$opend2d" ] && [ -f "$opend2d/crates/d2d-ffi/Cargo.toml" ] || { echo "usage: build-native.sh <opend2d-worktree>" >&2; exit 1; }
opend2d="$(cd "$opend2d" && pwd)"

# Emscripten toolchain from the installed wasm-tools workload.
root="${DOTNET_ROOT:-$HOME/.dotnet}"
em="$(ls -d "$root"/packs/Microsoft.NET.Runtime.Emscripten.3.*.Sdk.*/*/tools 2>/dev/null | head -1)"
cache="$(ls -d "$root"/packs/Microsoft.NET.Runtime.Emscripten.3.*.Cache.*/*/tools/emscripten/cache 2>/dev/null | head -1)"
node="$(ls "$root"/packs/Microsoft.NET.Runtime.Emscripten.3.*.Node.*/*/tools/bin/node 2>/dev/null | head -1)"
[ -n "$em" ] && [ -n "$node" ] || { echo "emscripten packs not found under $root/packs (install the wasm-tools workload)" >&2; exit 1; }
export EMSDK_PATH="$em" DOTNET_EMSCRIPTEN_LLVM_ROOT="$em/bin" DOTNET_EMSCRIPTEN_NODE_JS="$node" \
       DOTNET_EMSCRIPTEN_BINARYEN_ROOT="$em" EM_CONFIG="$em/emscripten/.emscripten" EM_CACHE="$cache" \
       FROZEN_CACHE=True PATH="$em/emscripten:$em/bin:$(dirname "$node"):$PATH"

out="$here/.build/native-wasm"
# target-cpu=mvp keeps the archive to the feature set the .NET wasm pack's tooling accepts.
RUSTFLAGS="-Ctarget-cpu=mvp" CARGO_TARGET_DIR="$out" \
    cargo rustc --manifest-path "$opend2d/Cargo.toml" -p d2d-ffi --lib --crate-type staticlib \
        --release --target wasm32-unknown-emscripten
rel="$out/wasm32-unknown-emscripten/release"
cp "$rel/libd2d1.a" "$rel/d2d1.a"
rm -f "$rel/dwrite.a" && emar rcs "$rel/dwrite.a"
echo "built -> $rel/d2d1.a ($(wc -c < "$rel/d2d1.a") bytes), $rel/dwrite.a (module name only)"
