#!/bin/sh
# A pinned build-time download: each URL in turn (the same file on another host, for when one cannot be reached),
# each with long retries, until one gives the expected SHA-256. Any source is safe, as only those bytes are kept.
# Usage: fetch.sh <out file> <sha256> <url>...
set -eu
[ "$#" -ge 3 ] || { echo 'usage: fetch.sh <out file> <sha256> <url>...' >&2; exit 2; }
out=$1 sha256=$2
shift 2
# shasum where there is one: macOS's own sha256sum (BSD) checks no list on stdin, and Alpine has only sha256sum.
if command -v shasum >/dev/null; then sum='shasum -a 256'; else sum=sha256sum; fi
for url do
    if curl --retry 5 --retry-delay 10 --retry-all-errors -sSfLo "$out" "$url" && echo "$sha256  $out" | $sum -c; then
        exit 0
    fi
    echo "fetch.sh: $url gave no file with that SHA-256; trying the next source" >&2
done
rm -f "$out"
echo "fetch.sh: no source gave $out with SHA-256 $sha256" >&2
exit 1
