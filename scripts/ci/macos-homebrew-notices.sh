#!/bin/sh
# The licence notices of the Homebrew libraries the macOS bundle carries beside the plug-in host (pdn/): each into
# pdn/notices/homebrew/<formula>-<version>/, listed in pdn/notices/README.md. A formula's keg usually holds them
# (Homebrew copies the source's top-level COPYING, LICENSE and the like). A formula that keeps them elsewhere in its
# source (glib's LICENSES/) gets them from that source, which brew fetch checks against the formula's SHA-256.
# Fails, naming every library left without a licence, so one run shows them all.
# Usage: macos-homebrew-notices.sh <pdn dir>
set -eu
[ "$#" -eq 1 ] || { echo 'usage: macos-homebrew-notices.sh <pdn dir>' >&2; exit 2; }
pdn=$1
prefix=$(brew --prefix)
missing=
for lib in "$pdn"/*.dylib; do
    brewlib="$prefix/lib/$(basename "$lib")"
    [ -e "$brewlib" ] || continue   # not from Homebrew (opend2d, libgdiplus, libuser32)
    keg=$(cd "$(dirname "$(realpath "$brewlib")")/.." && pwd)   # Cellar/<formula>/<version>
    formula=$(basename "$(dirname "$keg")")
    version=$(basename "$keg")
    notice="$pdn/notices/homebrew/$formula-$version"
    if [ ! -d "$notice" ]; then
        mkdir -p "$notice"
        find "$keg" -maxdepth 4 -type f \( -iname 'copying*' -o -iname 'licen[cs]e*' -o -iname 'notice*' \
            -o -iname 'authors*' -o -path '*/LICENSES/*' \) -exec cp {} "$notice/" \;
        if ! ls "$notice"/* >/dev/null 2>&1; then
            brew fetch --build-from-source --quiet "$formula" >/dev/null
            src=$(brew --cache --build-from-source "$formula")
            # the source's top-level licence files and its LICENSES/ folder, flattened into the notice folder
            list=$(mktemp)
            tar -tf "$src" | grep -iE '^[^/]+/((copying|licen[cs]e|notice)[^/]*|LICENSES/[^/]+)$' > "$list" || true
            if [ -s "$list" ]; then
                tmp=$(mktemp -d)
                tar -xf "$src" -C "$tmp" -T "$list"
                find "$tmp" -type f -exec cp {} "$notice/" \;
                rm -rf "$tmp"
            fi
            rm -f "$list"
        fi
    fi
    if ls "$notice"/* >/dev/null 2>&1; then
        echo "- $(basename "$lib") (Homebrew $formula $version, in homebrew/)" >> "$pdn/notices/README.md"
    else
        missing="$missing $(basename "$lib") ($formula $version)"
    fi
done
[ -z "$missing" ] || { echo "no licence found for:$missing" >&2; exit 1; }
