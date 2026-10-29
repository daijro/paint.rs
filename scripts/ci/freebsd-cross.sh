#!/bin/sh
# The FreeBSD release job's cross toolchain: GitHub has no FreeBSD runners, so a Linux runner builds the
# x86_64 FreeBSD download with clang and lld against a pinned FreeBSD base system, checked by SHA-256. The
# oldest supported release, so the download runs on it and every later one.
# Usage: freebsd-cross.sh <work dir>. It leaves the sysroot in <work dir>/freebsd; the compiler wrappers (cc,
# c++ and lld-clang, which links with lld), a meson cross file and a CMake toolchain file in
# <work dir>/freebsd-bin; and points cargo, cc, cmake and pkg-config at them, in GitHub Actions through
# GITHUB_ENV, else in <work dir>/freebsd.env.
set -eu
[ "$#" -eq 1 ] || { echo 'usage: freebsd-cross.sh <work dir>' >&2; exit 2; }
version=14.4
sha256=769f60a6eea2938ad6b7943cfbcc17dfaf7d1f7ba59a32b869a00349302df853
triple=x86_64-unknown-freebsd$version
work=$(cd -- "$1" && pwd)
sysroot=$work/freebsd
bin=$work/freebsd-bin
mkdir -p "$sysroot" "$bin"
curl --retry 3 -sSfLo "$work/base.txz" "https://download.freebsd.org/releases/amd64/$version-RELEASE/base.txz"
echo "$sha256  $work/base.txz" | sha256sum -c
# Headers, libraries and the C runtime objects, without the debug files.
tar -xJf "$work/base.txz" -C "$sysroot" --exclude ./usr/lib/debug ./lib ./usr/lib ./usr/include
# jpegxl-src picks its C++ library with cfg! in a build script, so by the build machine's system: a Linux
# runner asks for libstdc++, which FreeBSD's is libc++.
echo 'INPUT(-lc++)' > "$sysroot/usr/lib/libstdc++.so"
# -fuse-ld=lld only where they link: meson's checks make an unused argument an error. rustc reads a linker
# named *-clang as a compiler driver.
for tool in cc:clang c++:clang++ lld-clang:'clang -fuse-ld=lld'; do
    printf '#!/bin/sh\nexec %s --target=%s --sysroot=%s "$@"\n' "${tool#*:}" "$triple" "$sysroot" > "$bin/${tool%%:*}"
    chmod +x "$bin/${tool%%:*}"
done
cat > "$bin/meson-cross.ini" <<EOF
[binaries]
c = '$bin/cc'
c_ld = 'lld'
ar = 'llvm-ar'
strip = 'llvm-strip'
nasm = 'nasm'

[host_machine]
system = 'freebsd'
cpu_family = 'x86_64'
cpu = 'x86_64'
endian = 'little'
EOF
cat > "$bin/toolchain.cmake" <<EOF
set(CMAKE_SYSTEM_NAME FreeBSD)
set(CMAKE_SYSTEM_PROCESSOR x86_64)
set(CMAKE_C_COMPILER $bin/cc)
set(CMAKE_CXX_COMPILER $bin/c++)
set(CMAKE_SYSROOT $sysroot)
set(CMAKE_EXE_LINKER_FLAGS_INIT -fuse-ld=lld)
set(CMAKE_SHARED_LINKER_FLAGS_INIT -fuse-ld=lld)
set(CMAKE_MODULE_LINKER_FLAGS_INIT -fuse-ld=lld)
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
EOF
# pkg-config reads only build-dav1d.sh's FreeBSD dav1d (meson puts its .pc in libdata/pkgconfig there),
# never the runner's own libraries.
cat >> "${GITHUB_ENV:-$work/freebsd.env}" <<EOF
CC_x86_64_unknown_freebsd=$bin/cc
CXX_x86_64_unknown_freebsd=$bin/c++
AR_x86_64_unknown_freebsd=llvm-ar
CARGO_TARGET_X86_64_UNKNOWN_FREEBSD_LINKER=$bin/lld-clang
CMAKE_TOOLCHAIN_FILE_x86_64_unknown_freebsd=$bin/toolchain.cmake
PKG_CONFIG_ALLOW_CROSS=1
PKG_CONFIG_LIBDIR_x86_64_unknown_freebsd=$work/dav1d/libdata/pkgconfig
EOF
