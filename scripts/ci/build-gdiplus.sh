#!/bin/sh
# The .NET plugin host's GDI+ for the Linux and macOS release jobs. Most Paint.NET plugins touch
# System.Drawing (icons, drawing); off Windows that is System.Drawing.Common 6.0's unix build, the last
# that P/Invokes libgdiplus, over libgdiplus built from mono/libgdiplus 6.1 (pdn-host/README.md "GDI+")
# with our libgdiplus.patch, and giflib and libjpeg-turbo linked in. All four are pinned and checked by
# SHA-256. Usage: build-gdiplus.sh <work dir>.
# It leaves <work dir>/libgdiplus.so (Linux) or .dylib (macOS), <work dir>/System.Drawing.Common.dll and
# the four licences in <work dir>/gdiplus-notice, and in GitHub Actions points pdn-host/bridge/publish.sh at them
# (PDN_LIBGDIPLUS, PDN_SDC_UNIX, PDN_GDIPLUS_NOTICE).
set -eu
[ "$#" -eq 1 ] || { echo 'usage: build-gdiplus.sh <work dir>' >&2; exit 2; }
commit=44b44f92836f035e12b27c718e1f7bd42e369e28   # libgdiplus 6.1
sha256=a1656e30bee9adc6c453e0df2ca3ce74210d1c00fdba99fcf082cac8392e5e43
sdc_version=6.0.0
sdc_sha256=ffd11a01b11e3a310b452319688992d4ef059947bc8f85ae154c3554eacfc80a
giflib_version=6.1.3
giflib_sha256=b65b66b99f0424b93525f987386f22fc5efb9da2bfc92ad4a532249aaffbab0e
jpeg_version=3.1.4.1
jpeg_sha256=ecae8008e2cc9ade2f2c1bb9d5e6d4fb73e7c433866a056bd82980741571a022
here=$(cd -- "$(dirname -- "$0")" && pwd)
cd -- "$1"

sh "$here/fetch.sh" libgdiplus.tar.gz "$sha256" "https://github.com/mono/libgdiplus/archive/$commit.tar.gz" \
    "https://codeload.github.com/mono/libgdiplus/tar.gz/$commit"
tar -xzf libgdiplus.tar.gz
# libgdiplus.patch, what plug-ins need of GDI+ that 6.1 does otherwise:
# - GraphicsPath.Widen left the path as it was, so a plug-in that outlines a selection by widening it (TechnoRobbo's
#   QuickOutLiner) filled nothing and stopped. It now outlines each figure with the pen's width, joins and caps.
# - A region's scans dropped a rectangle starting at x = -1.
# - A GIF is saved as Windows GDI+ lays one out (GIF89a, a 256-colour global table, a control extension before
#   each image), and one with a stray byte after its last image opens, as there: Simon B's Animated GIF builds
#   its files from that layout and ends them with such a byte.
patch -d "libgdiplus-$commit" -p1 < "$here/libgdiplus.patch"
# giflib (MIT) as a static library, linked into libgdiplus so GIFs load (some plugins keep their images
# as GIF resources) with no runtime dependency.
sh "$here/fetch.sh" giflib.tar.gz "$giflib_sha256" \
    "https://downloads.sourceforge.net/project/giflib/giflib-6.x/giflib-$giflib_version.tar.gz" \
    "https://ftp.osuosl.org/pub/blfs/conglomeration/giflib/giflib-$giflib_version.tar.gz"
tar -xzf giflib.tar.gz
make -C "giflib-$giflib_version" libgif.a
gif=$PWD/giflib
mkdir -p "$gif/include" "$gif/lib"
cp "giflib-$giflib_version/gif_lib.h" "$gif/include/"
cp "giflib-$giflib_version/libgif.a" "$gif/lib/"
# libjpeg-turbo (IJG, BSD-3-Clause and zlib licences) as a static library too, so plug-ins can save and open JPEG
# through GDI+ (the Paint Shop Pro FileType's thumbnail, Optimized JPEG) with no runtime dependency: the system
# libjpeg's soname differs between distributions (Fedora's libjpeg.so.62, Debian's .so.8). Its SIMD code is left
# out, so the build needs no assembler.
sh "$here/fetch.sh" libjpeg-turbo.tar.gz "$jpeg_sha256" \
    "https://github.com/libjpeg-turbo/libjpeg-turbo/releases/download/$jpeg_version/libjpeg-turbo-$jpeg_version.tar.gz" \
    "https://ftp.osuosl.org/pub/blfs/conglomeration/libjpeg-turbo/libjpeg-turbo-$jpeg_version.tar.gz"
tar -xzf libjpeg-turbo.tar.gz
jpeg=$PWD/libjpeg
cmake -S "libjpeg-turbo-$jpeg_version" -B libjpeg-build -DCMAKE_BUILD_TYPE=Release -DENABLE_SHARED=OFF -DENABLE_STATIC=ON \
    -DWITH_TURBOJPEG=OFF -DWITH_SIMD=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DCMAKE_INSTALL_PREFIX="$jpeg" -DCMAKE_INSTALL_LIBDIR=lib
cmake --build libjpeg-build --target install
# No EXIF or TIFF codecs: their sonames differ between distributions too, and plug-ins use GDI+ for PNG icons and
# drawing. What is left (cairo, pango, glib, libpng, fontconfig, freetype) every desktop has. Only src/ is built: the
# bundled gtest is not needed.
# On Linux the static libraries' symbols stay inside libgdiplus (--exclude-libs): another library in the process
# exporting jpeg_* (a system libjpeg an image loader brings in) must not take its calls. macOS's two-level
# namespace already binds them to the copy linked in.
# On macOS libgdiplus's configure skips libpng's pkg-config entry and looks for -lpng on the default paths, and its
# sources include <cairo/cairo.h>, <X11/Xlib.h> and the like from the directory above each library's own: Homebrew's
# include and lib directories, which are default paths on Intel (/usr/local) but not on Apple Silicon (/opt/homebrew).
# libpng is cairo's own, which the macOS job carries in the bundle beside libgdiplus.
case "$(uname -s)" in
    Darwin) hide= brew_cflags="-I$(brew --prefix)/include" brew_libs="-L$(brew --prefix)/lib" ;;
    *) hide='-Wl,--exclude-libs,libjpeg.a:libgif.a' brew_cflags= brew_libs= ;;
esac
(cd "libgdiplus-$commit" && NOCONFIGURE=1 ./autogen.sh \
    && ./configure --with-libgif="$gif" --with-libjpeg="$jpeg" CPPFLAGS="-I$gif/include -I$jpeg/include $brew_cflags" \
        LDFLAGS="-L$gif/lib -L$jpeg/lib $brew_libs $hide" \
        --without-libexif --without-libtiff \
    && make -C src)
grep -q 'define HAVE_LIBGIF 1' "libgdiplus-$commit/config.h"    # configure found the static giflib
grep -q 'define HAVE_LIBJPEG 1' "libgdiplus-$commit/config.h"   # and the static libjpeg-turbo
case "$(uname -s)" in
    Darwin) cp "libgdiplus-$commit/src/.libs/libgdiplus.0.dylib" libgdiplus.dylib; lib=libgdiplus.dylib ;;
    *) cp "libgdiplus-$commit/src/.libs/libgdiplus.so.0.0.0" libgdiplus.so; lib=libgdiplus.so ;;
esac
{ echo 'libgdiplus (mono/libgdiplus):'; cat "libgdiplus-$commit/LICENSE"
  printf '\ngiflib %s, linked into libgdiplus:\n' "$giflib_version"; cat "giflib-$giflib_version/COPYING"
  printf '\nlibjpeg-turbo %s, linked into libgdiplus:\n' "$jpeg_version"; cat "libjpeg-turbo-$jpeg_version/LICENSE.md" "libjpeg-turbo-$jpeg_version/README.ijg"; } > gdiplus-notice

sh "$here/fetch.sh" sdc.nupkg "$sdc_sha256" \
    "https://api.nuget.org/v3-flatcontainer/system.drawing.common/$sdc_version/system.drawing.common.$sdc_version.nupkg" \
    "https://www.nuget.org/api/v2/package/System.Drawing.Common/$sdc_version"
unzip -q -o -j sdc.nupkg runtimes/unix/lib/net6.0/System.Drawing.Common.dll -d .
{ printf '\nSystem.Drawing.Common %s (.NET):\n' "$sdc_version"; unzip -p sdc.nupkg LICENSE.TXT; } >> gdiplus-notice
grep -q 'Permission is hereby granted' gdiplus-notice

echo "PDN_LIBGDIPLUS=$PWD/$lib" >> "${GITHUB_ENV:-/dev/null}"
echo "PDN_SDC_UNIX=$PWD/System.Drawing.Common.dll" >> "${GITHUB_ENV:-/dev/null}"
echo "PDN_GDIPLUS_NOTICE=$PWD/gdiplus-notice" >> "${GITHUB_ENV:-/dev/null}"
