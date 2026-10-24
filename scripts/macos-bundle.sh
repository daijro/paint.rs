#!/bin/sh
# Task56/R116: stage the minimal Finder/Dock application. Build/sign/notarize instructions are Task58.
set -eu
if [ "$#" -ne 2 ]; then
    echo 'usage: macos-bundle.sh /path/to/macos/paintrs /new/path/paint.rs.app' >&2
    exit 2
fi
binary=$1
bundle=$2
[ -f "$binary" ] && [ -x "$binary" ] || { echo 'input must be an executable macOS paintrs binary' >&2; exit 2; }
[ ! -e "$bundle" ] || { echo 'destination already exists' >&2; exit 2; }
case "$bundle" in *.app) ;; *) echo 'destination must end in .app' >&2; exit 2;; esac
source_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
# Info.plist CFBundleIconFile: AppIcon.icns from assets/app-icon.png (1024 px, rendered from
# assets/app-icon.svg; a paintrs unit test keeps it in sync) with the macOS sips and iconutil,
# built in a temporary directory first so a failure leaves no half-built bundle.
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
mkdir -- "$work/AppIcon.iconset"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$source_dir/assets/app-icon.png" --out "$work/AppIcon.iconset/icon_${size}x${size}.png" >/dev/null
    sips -z "$((size * 2))" "$((size * 2))" "$source_dir/assets/app-icon.png" --out "$work/AppIcon.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns -o "$work/AppIcon.icns" "$work/AppIcon.iconset"
mkdir -p -- "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp -- "$source_dir/packaging/macos/Info.plist" "$bundle/Contents/Info.plist"
cp -- "$binary" "$bundle/Contents/MacOS/paintrs"
chmod 755 "$bundle/Contents/MacOS/paintrs"
cp -- "$work/AppIcon.icns" "$bundle/Contents/Resources/AppIcon.icns"
