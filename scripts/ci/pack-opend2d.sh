#!/bin/sh
# Make the opend2d source snapshot the release jobs build (scripts/ci/fetch-opend2d.sh): only what paint.rs
# and its drop-in build need (Cargo.toml, Cargo.lock, the crates, LICENSE, THIRD-PARTY-NOTICES), plus the
# harness's manifest and lib.rs — a dev-dependency cargo must resolve in opend2d's workspace, never built —
# and not its capture tools, test scripts, conformance data or notes. Usage: pack-opend2d.sh <opend2d repo> <commit> <out dir>.
# Prints the SHA-256 to pin in fetch-opend2d.sh and the upload command; the archive is reproducible
# (git archive of one commit, gzip without a timestamp).
set -eu
[ "$#" -eq 3 ] || { echo 'usage: pack-opend2d.sh <opend2d repo> <commit> <out dir>' >&2; exit 2; }
commit=$(git -C "$1" rev-parse --verify "$2^{commit}")
out="$3/opend2d-$commit.tar.gz"
git -C "$1" archive --format=tar --prefix="opend2d-$commit/" "$commit" Cargo.toml Cargo.lock LICENSE THIRD-PARTY-NOTICES \
    crates ':(exclude)crates/d2d-types/gen' ':(exclude,glob)crates/**/*.py' ':(exclude,glob)crates/**/*.sh' \
    ':(exclude,glob)crates/**/*.c' ':(exclude,glob)crates/**/*.cpp' \
    harness/Cargo.toml harness/src/lib.rs | gzip -n -9 > "$out"
if command -v shasum >/dev/null; then sum=$(shasum -a 256 "$out"); else sum=$(sha256sum "$out"); fi
echo "commit=$commit"
echo "sha256=${sum%% *}"
echo "upload: gh release upload opend2d-snapshots '$out' --repo daijro/paint.rs"
