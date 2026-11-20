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

# The stand-ins PaintNetWeb.csproj builds through MSBuild tasks (Microsoft.Win32.Registry, WindowsBase,
# PresentationCore, PresentationFramework) are not restored by its own restore: restore them first, or a fresh
# checkout cannot publish.
for p in Win32Registry WindowsBase PresentationCore PresentationFramework; do
    "$here/../paintnet/dn.sh" restore "$here/../paintnet/assemblies/$p/$p.csproj" \
        --disable-build-servers -nodeReuse:false >/dev/null
done
"$here/../paintnet/dn.sh" publish "$here/app/PaintNetWeb.csproj" -c Release \
    -m:1 --disable-build-servers -nodeReuse:false

bundle="$here/app/bin/Release/net10.0/browser-wasm/AppBundle"
[ -d "$bundle" ] || { echo "no AppBundle at $bundle" >&2; exit 1; }

rm -rf "$dest"
mkdir -p "$dest"
cp -r "$bundle/." "$dest/"

# gzip the .wasm (runtime + assemblies) that the page fetches through index.html's loader.
find "$dest" -name '*.wasm' -exec sh -c 'gzip -9 -c "$1" > "$1.gz"' _ {} \;

# Licence notices for what pdn/ ships, into pdn/notices/: the .NET runtime and the NuGet packages (the publish's
# deps file), emscripten's runtime and C library, Mono.Cecil, Roslyn, and with $OPEND2D_DIR the Rust crates in OpenD2D's
# drop-ins (--strict then, as for a release).
root="${DOTNET_ROOT:-$HOME/.dotnet}"
python3 "$here/../bridge/collect-notices.py" "$dest" \
    --deps "$here/app/bin/Release/net10.0/browser-wasm/PaintNetWeb.deps.json" --nuget "$here/../paintnet/.build/nuget" \
    --packs "$root/packs" --emscripten "$(ls -d "$root"/packs/Microsoft.NET.Runtime.Emscripten.3.*.Sdk.*/*/tools/emscripten | head -n 1)" \
    ${OPEND2D_DIR:+--opend2d "$OPEND2D_DIR" --strict}

echo "staged -> $dest ($(du -sh "$dest" | cut -f1))"
