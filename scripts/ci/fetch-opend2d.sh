#!/bin/sh
# CI's opend2d, from a pinned source snapshot checked by SHA-256. pdn-core builds on its d2d, crt and
# warpmath crates, and the .NET plugin host ships its Direct2D/DirectWrite/WIC drop-ins (libd2d1,
# libdwrite, libwindowscodecs) on Linux and macOS. opend2d's repository is github.com/daijro/opend2d;
# scripts/ci/pack-opend2d.sh makes the snapshot of one commit, which is attached to an OpenD2D release.
# Usage: fetch-opend2d.sh <work dir> (the download goes there). The source is unpacked where Cargo
# resolves pdn-core's path dependencies: crates/pdn-core's ../../../../../opend2d, i.e. <repo>/../../../
# opend2d (the layout of a paint.rs worktree beside an opend2d checkout). In GitHub Actions OPEND2D_DIR
# then points pdn-host/bridge/publish.sh at it. It never replaces an existing directory there.
set -eu
[ "$#" -eq 1 ] || { echo 'usage: fetch-opend2d.sh <work dir>' >&2; exit 2; }
release=v0.1.0-beta.1   # the OpenD2D release the snapshot is attached to
commit=f699e0378c30988bb31d65e5c771a464a6d7fe4b
sha256=e28186161f8c377d8cd4a7c0230c4143366f069d3bae2096801af9e8176178ca
url="${OPEND2D_SNAPSHOT_URL:-https://github.com/daijro/opend2d/releases/download/$release/opend2d-$commit.tar.gz}"
dest="$(cd "$(dirname "$0")/../.." && pwd)/../../../opend2d"
[ ! -e "$dest" ] || { echo "error: $dest already exists; not replacing it" >&2; exit 1; }
cd -- "$1"
curl --retry 3 -sSfLo opend2d.tar.gz "$url"
if command -v shasum >/dev/null; then sum='shasum -a 256'; else sum=sha256sum; fi
echo "$sha256  opend2d.tar.gz" | $sum -c
mkdir -p "$dest" && dest="$(cd "$dest" && pwd)"
tar -xzf opend2d.tar.gz -C "$dest" --strip-components=1
echo "opend2d $commit -> $dest"
echo "OPEND2D_DIR=$dest" >> "${GITHUB_ENV:-/dev/null}"
