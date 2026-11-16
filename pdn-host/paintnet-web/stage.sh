#!/usr/bin/env sh
# Stage the Paint.NET browser host into paint.rs's web site so plug-ins load at run time.
#
#   ./stage.sh <site-dir>     publish app/ and copy its AppBundle to <site-dir>/pdn
#
# paint.rs's pdn_web.js lazily imports <site-dir>/pdn/_framework/dotnet.js the first time a
# plug-in is installed (nothing of the ~MB runtime is fetched before that). Run this after
# `trunk build` / tools/web-dist.sh, which clears dist/. A .gz copy is written beside every
# .wasm so index.html's fetch loader serves it compressed (it falls back to the plain file
# when a .gz is missing, so skipping gzip only costs bytes, not correctness).
#
# Needs the .NET SDK with the wasm-tools workload (see README.md).
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
site="${1:?usage: stage.sh <site-dir>}"
dest="$site/pdn"

"$here/../paintnet/dn.sh" publish "$here/app/PaintNetWeb.csproj" -c Release \
    -m:1 --disable-build-servers -nodeReuse:false

bundle="$here/app/bin/Release/net10.0/browser-wasm/AppBundle"
[ -d "$bundle" ] || { echo "no AppBundle at $bundle" >&2; exit 1; }

rm -rf "$dest"
mkdir -p "$dest"
cp -r "$bundle/." "$dest/"

# gzip the .wasm (runtime + assemblies) that the page fetches through index.html's loader.
find "$dest" -name '*.wasm' -exec sh -c 'gzip -9 -c "$1" > "$1.gz"' _ {} \;

echo "staged -> $dest ($(du -sh "$dest" | cut -f1))"
