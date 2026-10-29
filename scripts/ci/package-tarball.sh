#!/bin/sh
# The Linux, Alpine and FreeBSD release jobs' tarball: the binary, the licences and notices, the fonts, and
# scripts/linux-install-desktop.sh with what it installs (the launcher entry, for any XDG desktop).
# Usage: package-tarball.sh <dist name> <paintrs binary> <dav1d notice>, from the repository root. It leaves
# the directory <dist name> and <dist name>.tar.gz. PYTHON is the notices' interpreter (3.11 or newer).
set -eu
[ "$#" -eq 3 ] || { echo 'usage: package-tarball.sh <dist name> <paintrs binary> <dav1d notice>' >&2; exit 2; }
dist=$1
mkdir -p "$dist/scripts" "$dist/packaging/linux" "$dist/assets"
cp "$2" "$dist/paintrs"
cp LICENSE LICENSE.md NOTICE.md "$dist/"
cp scripts/linux-install-desktop.sh "$dist/scripts/"
cp packaging/linux/paint-rs.desktop packaging/linux/paint-rs.xml "$dist/packaging/linux/"
cp assets/app-icon.svg assets/app-icon.png "$dist/assets/"
cp -R assets/font assets/fonts "$dist/assets/"
test -f "$dist/assets/font/carlito/Carlito-Regular.ttf"
test -f "$dist/assets/fonts/noto-color-emoji/NotoColorEmoji.ttf"
cargo +1.99.1 metadata --locked --format-version 1 > metadata.json
"${PYTHON:-python3}" scripts/ci/collect_notices.py "$dist/notices" --kind native --metadata metadata.json --dav1d-license "$3"
tar -czf "$dist.tar.gz" "$dist"
