#!/bin/sh
# CI's opend2d, from a pinned source snapshot checked by SHA-256. pdn-core builds on its d2d, crt and
# warpmath crates, and the .NET plugin host ships its Direct2D/DirectWrite/WIC drop-ins (libd2d1,
# libdwrite, libwindowscodecs) on Linux and macOS. opend2d's repository is github.com/daijro/opend2d;
# scripts/ci/pack-opend2d.sh makes the snapshot of one commit, which is attached to an OpenD2D release.
# Usage: fetch-opend2d.sh <work dir> (the download goes there). The source is unpacked where Cargo
# resolves pdn-core's path dependencies: crates/pdn-core's ../../../../../opend2d, i.e. <repo>/../../../
# opend2d (the layout of a paint.rs worktree beside an opend2d checkout). In GitHub Actions OPEND2D_DIR
# then points pdn-host/bridge/publish.sh at it. It never replaces an existing directory there.
# Keep commit= the snapshot crates/pdn-core/Cargo.toml fetches as a git dependency (push-opend2d-snapshot.sh).
set -eu
[ "$#" -eq 1 ] || { echo 'usage: fetch-opend2d.sh <work dir>' >&2; exit 2; }
release=v0.1.0-beta.1   # the OpenD2D release the snapshot is attached to
commit=55baec1d5ed76e0e572dd795434a3f94edaeaf18
sha256=04fa161ce38f1d0bf656ecae094563de3b0831a0fe5be748c78668d02c4298a4
url="${OPEND2D_SNAPSHOT_URL:-https://github.com/daijro/opend2d/releases/download/$release/opend2d-$commit.tar.gz}"
dest="$(cd "$(dirname "$0")/../.." && pwd)/../../../opend2d"
[ ! -e "$dest" ] || { echo "error: $dest already exists; not replacing it" >&2; exit 1; }
here=$(cd -- "$(dirname -- "$0")" && pwd)
cd -- "$1"
sh "$here/fetch.sh" opend2d.tar.gz "$sha256" "$url"
mkdir -p "$dest" && dest="$(cd "$dest" && pwd)"
tar -xzf opend2d.tar.gz -C "$dest" --strip-components=1
echo "opend2d $commit -> $dest"
echo "OPEND2D_DIR=$dest" >> "${GITHUB_ENV:-/dev/null}"
