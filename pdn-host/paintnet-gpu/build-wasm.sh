#!/usr/bin/env sh
# Build the GPU shader bridge as a wasm static archive for the browser host.
#
#   ./build-wasm.sh [opend2d-worktree]
#
# The only native symbol the browser GPU path imports is d2dgpu_render (Direct2D1Gpu.cs,
# [LibraryImport("d2dgpu")]); the DeviceContext is CPU-backed managed. So the web seam needs just
# libd2dgpu.a (it bundles opend2d's d2d-shader). We emit it as d2dgpu.a — Mono-wasm keys its P/Invoke
# table by the archive's file name, so it must match the import module "d2dgpu" (not "libd2dgpu").
# PaintNetWeb.csproj links it with <NativeFileReference> and the .NET wasm publish relinks it into
# dotnet.native.wasm.
#
# Prereqs: the .NET wasm-tools workload (for the emscripten target) and rustup's
# wasm32-unknown-emscripten std (`rustup target add wasm32-unknown-emscripten`). Build opend2d from a
# fresh `git worktree add` of its main, never the main checkout.
#
# NOTE: the publish's native relink runs wasm-opt; the workload ships binaryen 117, too old for the
# feature flags Rust's LLVM stamps (`--enable-bulk-memory-opt` …). Put a newer wasm-opt (binaryen
# 133) on the emscripten Sdk pack's bin/ first. Even then the interpreter aborts at the actual
# d2dgpu_render call (aot-runtime-wasm.c:188) — a Rust-LLVM/Mono wasm feature-set skew still open;
# see paintnet-web/README.md.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
opend2d="${1:-${OPEND2D_DIR:-}}"
[ -n "$opend2d" ] || { echo "usage: build-wasm.sh <opend2d-worktree>  (or set OPEND2D_DIR)" >&2; exit 1; }
opend2d="$(cd "$opend2d" && pwd)"   # absolute, so the .build/opend2d anchor is never self-referential
[ -d "$opend2d/crates/d2d-shader" ] || { echo "opend2d worktree not found at $opend2d" >&2; exit 1; }
mkdir -p "$here/.build"
ln -sfn "$opend2d" "$here/.build/opend2d"

# Emscripten toolchain from the installed wasm-tools workload (DOTNET_ROOT defaults to ~/.dotnet).
root="${DOTNET_ROOT:-$HOME/.dotnet}"
em="$(ls -d "$root"/packs/Microsoft.NET.Runtime.Emscripten.*.Sdk.*/*/tools 2>/dev/null | head -1)"
cache="$(ls -d "$root"/packs/Microsoft.NET.Runtime.Emscripten.*.Cache.*/*/tools/emscripten/cache 2>/dev/null | head -1)"
node="$(ls "$root"/packs/Microsoft.NET.Runtime.Emscripten.*.Node.*/*/tools/bin/node 2>/dev/null | head -1)"
[ -n "$em" ] && [ -n "$node" ] || { echo "emscripten packs not found under $root/packs (install the wasm-tools workload)" >&2; exit 1; }
export EMSDK_PATH="$em" DOTNET_EMSCRIPTEN_LLVM_ROOT="$em/bin" DOTNET_EMSCRIPTEN_NODE_JS="$node" \
       DOTNET_EMSCRIPTEN_BINARYEN_ROOT="$em" EM_CONFIG="$em/emscripten/.emscripten" EM_CACHE="$cache" \
       FROZEN_CACHE=True PATH="$em/emscripten:$em/bin:$(dirname "$node"):$PATH"

out="$here/.build/native-wasm"
# target-cpu=mvp keeps the archive to the feature set the .NET wasm pack's tooling accepts.
RUSTFLAGS="-Ctarget-cpu=mvp" CARGO_TARGET_DIR="$out" \
    cargo rustc --manifest-path "$here/native/d2dgpu/Cargo.toml" \
        --lib --crate-type staticlib --release --target wasm32-unknown-emscripten
rel="$out/wasm32-unknown-emscripten/release"
cp "$rel/libd2dgpu.a" "$rel/d2dgpu.a"
echo "built -> $rel/d2dgpu.a ($(wc -c < "$rel/d2dgpu.a") bytes; symbol d2dgpu_render)"
