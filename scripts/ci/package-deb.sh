#!/bin/sh
# The Linux release job's .deb, from the AppImage's AppDir: the same binary, .NET plugin host, desktop
# file, icons, MIME type, fonts and notices. paintrs looks for its plugin host (pdn/) and fonts (assets/)
# beside its own resolved path, so all three go in /usr/lib/paintrs and /usr/bin/paintrs links there.
# The desktop, icon and MIME caches update through their packages' dpkg triggers.
# Usage: package-deb.sh <AppDir> <out.deb>, from the repository root. Needs dpkg-deb 1.19 or newer.
set -eu
[ "$#" -eq 2 ] || { echo 'usage: package-deb.sh <AppDir> <out.deb>' >&2; exit 2; }
umask 022
app=$1/usr
version=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
[ -n "$version" ] || { echo 'error: no [workspace.package] version in Cargo.toml' >&2; exit 1; }
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
chmod 755 "$root"
lib=$root/usr/lib/paintrs
doc=$root/usr/share/doc/paintrs
mkdir -p "$lib/assets" "$root/usr/bin" "$doc" "$root/DEBIAN"
cp "$app/bin/paintrs" "$lib/"
cp -R "$app/bin/pdn" "$lib/"
cp -R "$app/share/paint.rs/assets/font" "$app/share/paint.rs/assets/fonts" "$lib/assets/"
ln -s ../lib/paintrs/paintrs "$root/usr/bin/paintrs"
cp -R "$app/share/applications" "$app/share/icons" "$app/share/mime" "$root/usr/share/"
cp -R "$app/share/doc/paint.rs/." "$doc/"
cat > "$doc/copyright" <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: paint.rs
Source: https://github.com/daijro/paint.rs

Files: *
Copyright: 2026-present paint.rs contributors
License: GPL-3.0-or-later
 The full text is in LICENSE in this directory. The bundled fonts, libraries and plugin host
 keep their own licences: see NOTICE.md and notices/ here.
EOF
# Depends: what paintrs links on the glibc 2.35 baseline (dav1d is static), plus what it loads at run
# time and cannot start without: libxkbcommon (winit's keyboard on X11 and Wayland) and a GPU API for
# wgpu (Vulkan, or GL through EGL). Recommends: the rest it loads, of which a session needs one set:
# Wayland (libwayland-client, and libwayland-egl for GL) or X11 (winit opens libX11, libX11-xcb,
# libXcursor, libXi, libxcb and libxkbcommon-x11; not libXrandr or libGL); and what the plugin host's
# libgdiplus links, which only Paint.NET plugins need.
cat > "$root/DEBIAN/control" <<EOF
Package: paintrs
Version: $version
Architecture: $(dpkg --print-architecture)
Maintainer: daijro <daijro.dev@gmail.com>
Installed-Size: $(du -sk "$root/usr" | cut -f1)
Depends: libc6 (>= 2.35), libgcc-s1, libstdc++6, libxkbcommon0, libvulkan1 | libegl1
Recommends: libvulkan1, libegl1, libwayland-client0, libwayland-egl1, libx11-6, libx11-xcb1, libxcb1, libxcursor1, libxi6, libxkbcommon-x11-0, libcairo2, libpango-1.0-0, libpangocairo-1.0-0, libharfbuzz0b, libglib2.0-0, libfreetype6, libfontconfig1, libpng16-16
Section: graphics
Priority: optional
Homepage: https://github.com/daijro/paint.rs
Description: open source reimplementation of Paint.NET
 paint.rs is a clean-room reimplementation of the Paint.NET image editor,
 with layers, effects, adjustments and Paint.NET's tools. It runs Paint.NET
 plugins through its bundled .NET plugin host.
EOF
dpkg-deb --root-owner-group -Zxz --build "$root" "$2"
