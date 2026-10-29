#!/bin/sh
# The Alpine (musl) release job, run as root in an Alpine container with the repository as the working
# directory: Rust from rustup, a static dav1d (build-dav1d.sh), and paintrs linked to musl dynamically, as a
# static binary cannot load the X11, Wayland and GPU libraries at run time.
# Usage: alpine-build.sh <dist name>. It leaves <dist name>.tar.gz (package-tarball.sh).
set -eu
[ "$#" -eq 1 ] || { echo 'usage: alpine-build.sh <dist name>' >&2; exit 2; }
apk add --no-cache build-base cmake meson nasm pkgconf curl python3 linux-headers rustup
rustup-init -y -q --profile minimal --default-toolchain 1.99.1
. "$HOME/.cargo/env"
work=$(mktemp -d)
sh scripts/ci/build-dav1d.sh "$work"
export PKG_CONFIG_PATH="$work/dav1d/lib/pkgconfig" SYSTEM_DEPS_DAV1D_LINK=static
RUSTFLAGS='-C target-feature=-crt-static' cargo +1.99.1 build --locked --profile dist -p paintrs
sh scripts/ci/package-tarball.sh "$1" "${CARGO_TARGET_DIR:-target}/dist/paintrs" "$work/dav1d-notice"
