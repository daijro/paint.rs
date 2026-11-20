#!/bin/sh
# Publish an opend2d source snapshot (pack-opend2d.sh's .tar.gz) as a history-free commit under the tag
# snapshot-<commit>, so Cargo can fetch crates/pdn-core's d2d, crt and warpmath as a git dependency.
# Usage: push-opend2d-snapshot.sh <opend2d-COMMIT.tar.gz>. Prints the rev to pin in crates/pdn-core/Cargo.toml;
# it must be the same commit fetch-opend2d.sh pins, so the drop-ins and pdn-core build one OpenD2D.
set -eu
[ "$#" -eq 1 ] || { echo 'usage: push-opend2d-snapshot.sh <opend2d-COMMIT.tar.gz>' >&2; exit 2; }
commit=$(basename "$1" .tar.gz); commit=${commit#opend2d-}
dir=$(mktemp -d); trap 'rm -rf "$dir"' EXIT
tar -xzf "$1" -C "$dir" --strip-components=1
cd "$dir"
git init -q -b snapshot && git add -A
git commit -q -m "OpenD2D source snapshot of $commit"
tag="snapshot-$(printf %.8s "$commit")"
git tag "$tag" && git push -q https://github.com/daijro/opend2d.git "refs/tags/$tag"
echo "rev = \"$(git rev-parse HEAD)\"  (tag $tag; pin it for d2d, crt and warpmath in crates/pdn-core/Cargo.toml)"
