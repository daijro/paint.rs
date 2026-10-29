#!/bin/sh
# The web release (README "Web"): the whole site in dist/.
#   tools/web-dist.sh          trunk build --release, wasm-opt -O3, the notices, a gzip copy (`<file>.gz`)
#                              beside every wasm, JS, HTML, TTF and OTF file, then --check
#   tools/web-dist.sh --zip    the same, then the release zip: target/paintrs-web-<version>.zip (or under
#                              $CARGO_TARGET_DIR), dist/ in one paintrs-web-<version>/ folder
#   tools/web-dist.sh --check  no build: prints the size table, and fails when a budget is over, a .gz is
#                              missing or stale, or any file is over 25 MiB
# Needs trunk, wasm-opt (binaryen; or set $WASM_OPT), gzip, python3 and, for --zip, zip. Trunk's own variables
# pass through, e.g. TRUNK_OFFLINE=true or TRUNK_BUILD_PUBLIC_URL=/paintrs/.
set -eu
cd "$(dirname "$0")/.."

# Budgets in bytes: the first measured build (2026-10-24: wasm 20,126,899 raw and 7,414,217 gzipped, JS 24,566
# gzipped, first load 7,454,421) with about 10% headroom, ratcheted at 0.7.1 (2026-10-29), whose plug-in host
# and store grew the wasm 6.7% over 0.6.1: wasm 22,594,161 raw and 8,212,037 gzipped, first load 8,254,118,
# with about 5% headroom.
WASM_RAW=23700000 WASM_GZ=8650000 JS_GZ=40000 FIRST_LOAD=8700000
FILE_MAX=26214400 # 25 MiB, Cloudflare's limit per file
# The binaryen release this was measured with; another one works, with a note.
BINARYEN=version_133
# Rust 1.99's default wasm32 features, plus SIMD (.cargo/config.toml). wasm-opt reads them from the module's
# target_features section, which --strip-debug drops; without them it assumes the 2017 baseline and rejects e.g.
# memory.copy.
FEATURES="--enable-bulk-memory --enable-bulk-memory-opt --enable-call-indirect-overlong --enable-multivalue
  --enable-mutable-globals --enable-nontrapping-float-to-int --enable-reference-types --enable-sign-ext --enable-simd"

# What goes out compressed: the app's wasm, JS and HTML, and what it fetches at runtime (the codecs, the fonts).
# Not WOFF2, which is compressed already. Trunk's and the assets' names have no spaces.
gz_files() {
    find dist -type f \( -name '*.wasm' -o -name '*.js' -o -name '*.html' -o -name '*.ttf' -o -name '*.otf' \)
}
size() { wc -c <"$1" | tr -d ' '; }

build() {
    wasm_opt=${WASM_OPT:-wasm-opt}
    if ! command -v "$wasm_opt" >/dev/null; then
        echo "needs wasm-opt (binaryen $BINARYEN) on PATH, or WASM_OPT" >&2
        exit 1
    fi
    "$wasm_opt" --version | grep -q "($BINARYEN)" || echo "note: $("$wasm_opt" --version), measured with $BINARYEN" >&2
    # Cargo metadata for the notices below, and for the target directory: Trunk keeps wasm-bindgen's output there
    # (wasm-bindgen/release/) for every release build and copies all its snippets into dist/.
    # Emptied first, dist/ gets only this build's snippets.
    meta=${CARGO_TARGET_DIR:-target}/web-dist-metadata.json
    mkdir -p "$(dirname "$meta")"
    cargo metadata --locked --format-version 1 >"$meta"
    target_dir=$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["target_directory"])' "$meta")
    rm -rf "${target_dir:?}/wasm-bindgen"
    trunk build --release --locked
    # Only paint.rs's own wasm: the codecs' magick.wasm is a third-party release, shipped unmodified
    # (assets/codecs/README.md), and -O3 takes 0.2% off it.
    for f in dist/*.wasm; do
        # shellcheck disable=SC2086 # one flag per word
        "$wasm_opt" -O3 --strip-debug $FEATURES "$f" -o "$f"
    done
    # The licence notices, as CI's web job collects them.
    python3 scripts/ci/collect_notices.py dist/notices --kind web --metadata "$meta"
    # -n: no name or time inside, so the same file always gives the same bytes; -k keeps the file for clients and
    # hosts without gzip.
    gz_files | xargs -r gzip -9 -k -n -f
    cp deploy/_headers dist/
}

fail=0
over() { # what, bytes, budget
    if [ "$2" -gt "$3" ]; then
        echo "OVER: $1 is $2 bytes, budget $3"
        fail=1
    fi
}

check() {
    set -- dist/paintrs-*_bg.wasm
    if [ $# -ne 1 ] || [ ! -f "$1" ]; then
        echo "dist/ has no single paintrs-*_bg.wasm: run tools/web-dist.sh first"
        exit 1
    fi
    wasm=$1 js=${1%_bg.wasm}.js
    printf '%-72s %11s %11s\n' file raw gzip
    for f in $(gz_files | sort); do
        if [ ! -f "$f.gz" ] || [ "$f" -nt "$f.gz" ]; then
            echo "MISSING or stale: $f.gz"
            fail=1
        else
            printf '%-72s %11s %11s\n' "$f" "$(size "$f")" "$(size "$f.gz")"
        fi
    done
    [ $fail = 0 ] || return 0

    over "$wasm" "$(size "$wasm")" $WASM_RAW
    over "$wasm.gz" "$(size "$wasm.gz")" $WASM_GZ
    over "$js.gz" "$(size "$js.gz")" $JS_GZ
    # Before the first frame: the wasm, the JS (with wasm-bindgen's snippets) and the page. The codecs and fonts
    # load on first use, and so does Object Selection's MobileSAM worker (its wasm and JS, then the model's parts).
    first=0
    for f in dist/paintrs-*.wasm dist/paintrs-*.js dist/index.html $(find dist/snippets -name '*.js'); do
        first=$((first + $(size "$f.gz")))
    done
    printf '%-72s %23s\n' "first load (wasm, JS, snippets, HTML; gzipped)" $first
    over "the first load" $first $FIRST_LOAD
    # The worker fetches its JS and wasm itself (index.html's .gz loader is the page's): raw, or gzipped where the
    # host sends the .gz with Content-Encoding (nginx's gzip_static, Caddy's precompressed).
    worker=0 worker_gz=0
    for f in dist/sam_worker*.js dist/sam_worker_bg.wasm; do
        worker=$((worker + $(size "$f"))) worker_gz=$((worker_gz + $(size "$f.gz")))
    done
    model=$(cat dist/models/mobilesam/*.safetensors.* | wc -c | tr -d ' ')
    printf '%-72s %11s %11s\n' "MobileSAM when picked: worker (wasm, JS)" $worker $worker_gz
    printf '%-72s %23s\n' "MobileSAM when picked: the model's parts (once; the Cache API keeps it)" "$model"
    for f in $(find dist -type f); do over "$f" "$(size "$f")" $FILE_MAX; done
}

release_zip() {
    v=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
    out=${CARGO_TARGET_DIR:-target}/paintrs-web-$v
    rm -rf "$out" "$out.zip"
    cp -R dist "$out"
    (cd "$out/.." && zip -qr9 "paintrs-web-$v.zip" "paintrs-web-$v")
    rm -rf "$out"
    echo "wrote $out.zip"
}

case ${1:-} in
    "")
        build
        check
        ;;
    --zip)
        build
        check
        if [ $fail = 0 ]; then release_zip; fi
        ;;
    --check) check ;;
    *)
        sed -n '2,10p' "$0"
        exit 2
        ;;
esac
exit $fail
