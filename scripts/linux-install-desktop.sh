#!/bin/sh
# Installs paint.rs's desktop entry, icon and .pdn MIME type for the current user, so launchers
# list it and Wayland taskbars show its icon (they ignore the window icon and look up the desktop
# file named after the app id, paint-rs). The optional argument is the paintrs binary to launch;
# by default the entry runs `paintrs` from PATH.
set -eu
if [ "$#" -gt 1 ]; then
    echo 'usage: linux-install-desktop.sh [/path/to/paintrs]' >&2
    exit 2
fi
source_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
exec_line='Exec=paintrs %F'
if [ "$#" -eq 1 ]; then
    [ -f "$1" ] && [ -x "$1" ] || { echo "not an executable: $1" >&2; exit 2; }
    binary=$(CDPATH= cd -- "$(dirname -- "$1")" && pwd)/$(basename -- "$1")
    case "$binary" in *[!A-Za-z0-9/._+-]*) echo 'the binary path needs quoting; move it or use PATH' >&2; exit 2;; esac
    exec_line="Exec=$binary %F"
fi
mkdir -p "$data/applications" "$data/icons/hicolor/scalable/apps" \
    "$data/icons/hicolor/512x512@2/apps" "$data/mime/packages"
sed "s|^Exec=.*|$exec_line|" "$source_dir/packaging/linux/paint-rs.desktop" > "$data/applications/paint-rs.desktop"
cp -- "$source_dir/assets/app-icon.svg" "$data/icons/hicolor/scalable/apps/paint-rs.svg"
# 1024 px, the square render of the SVG (macos-bundle.sh uses it too).
cp -- "$source_dir/assets/app-icon.png" "$data/icons/hicolor/512x512@2/apps/paint-rs.png"
cp -- "$source_dir/packaging/linux/paint-rs.xml" "$data/mime/packages/paint-rs.xml"
if command -v update-mime-database >/dev/null; then update-mime-database "$data/mime"; fi
if command -v update-desktop-database >/dev/null; then update-desktop-database "$data/applications"; fi
touch "$data/icons/hicolor"
# Refresh a user icon cache only where one exists: a new one would hide icons installed later.
if [ -f "$data/icons/hicolor/icon-theme.cache" ] && command -v gtk-update-icon-cache >/dev/null; then
    gtk-update-icon-cache -q -t -f "$data/icons/hicolor" || true
fi
if command -v kbuildsycoca6 >/dev/null; then kbuildsycoca6 >/dev/null 2>&1 || true
elif command -v kbuildsycoca5 >/dev/null; then kbuildsycoca5 >/dev/null 2>&1 || true; fi
echo "Installed $data/applications/paint-rs.desktop ($exec_line)"
