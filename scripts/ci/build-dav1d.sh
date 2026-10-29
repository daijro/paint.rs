#!/bin/sh
# R244: the release jobs' dav1d (Linux and macOS), a pinned release checked by SHA-256 and built as a
# static library, so the downloads need no libdav1d from the system or Homebrew.
# Usage: build-dav1d.sh <work dir> [meson cross file]. It leaves the install prefix in <work dir>/dav1d and
# the notice in <work dir>/dav1d-notice, and in GitHub Actions points pkg-config and system-deps at the
# static library. A cross file (scripts/ci/freebsd-cross.sh's) builds it for another system.
set -eu
[ "$#" -eq 1 ] || [ "$#" -eq 2 ] || { echo 'usage: build-dav1d.sh <work dir> [meson cross file]' >&2; exit 2; }
cross=${2:+$(cd -- "$(dirname -- "$2")" && pwd)/$(basename -- "$2")}
version=1.5.4
sha256=686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5
cd -- "$1"
curl --retry 3 -sSfLo dav1d.tar.xz "https://downloads.videolan.org/pub/videolan/dav1d/$version/dav1d-$version.tar.xz"
# macOS has only shasum, Alpine only sha256sum.
if command -v sha256sum >/dev/null; then sum=sha256sum; else sum='shasum -a 256'; fi
echo "$sha256  dav1d.tar.xz" | $sum -c
tar -xJf dav1d.tar.xz
# The notice: dav1d's COPYING, then the ISC header of x86inc.asm (x264 project), which the x86 assembly
# is built with.
{ cat "dav1d-$version/COPYING"; printf '\nsrc/ext/x86/x86inc.asm:\n'
  awk '!/^;\*/ {exit} {print}' "dav1d-$version/src/ext/x86/x86inc.asm"; } > dav1d-notice
grep -q 'Permission to use, copy, modify' dav1d-notice
meson setup dav1d-build "dav1d-$version" --prefix "$PWD/dav1d" --libdir lib -Ddefault_library=static --wrap-mode=nodownload \
    -Denable_tools=false -Denable_tests=false ${cross:+--cross-file "$cross"}
ninja -C dav1d-build install
echo "PKG_CONFIG_PATH=$PWD/dav1d/lib/pkgconfig" >> "${GITHUB_ENV:-/dev/null}"
echo SYSTEM_DEPS_DAV1D_LINK=static >> "${GITHUB_ENV:-/dev/null}"
