#!/usr/bin/env python3
"""tools/web-dist.sh, after gzip: the wasm files go out only as their .gz, which index.html's loader fetches and
gunzips. The plain .wasm is left out (a host's per-file limit applies to it too), except the MobileSAM worker's,
which its worker fetches itself. A .gz over the limit goes out in parts, <file>.gz.0, .1, … of at most the limit
each; index.html's loader fetches them together and joins them in order. This writes each split file's part count
into dist/index.html (`const parts = {…};`) and preloads its parts in place of the .gz.
   gz-parts.py <dist> <per-file limit in bytes>"""
import json
import re
import sys
from pathlib import Path

WORKER = {'sam_worker_bg.wasm'}
dist, cap = Path(sys.argv[1]), int(sys.argv[2])
parts = {}
for wasm in sorted(dist.rglob('*.wasm')):
    if wasm.name in WORKER:
        continue
    gz = wasm.with_name(wasm.name + '.gz')
    if not gz.is_file():
        sys.exit(f'{gz} is missing')
    wasm.unlink()
    data = gz.read_bytes()
    if len(data) <= cap:
        continue
    for i in range(0, len(data), cap):
        gz.with_name(f'{gz.name}.{i // cap}').write_bytes(data[i:i + cap])
    gz.unlink()
    parts[wasm.name] = -(-len(data) // cap)

html = dist / 'index.html'
page = html.read_text()
if page.count('const parts = {};') != 1:
    sys.exit(f'{html} has no single `const parts = {{}};` for the loader')
page = page.replace('const parts = {};', f'const parts = {json.dumps(parts, separators=(",", ":"))};')
for name, n in parts.items():
    # Trunk's preload (Trunk.toml pattern_preload) names the .gz: preload the parts instead.
    page = re.sub(rf'<link rel="preload" href="([^"]*{re.escape(name)}\.gz)"([^>]*)>',
                  lambda m: ''.join(f'<link rel="preload" href="{m[1]}.{i}"{m[2]}>' for i in range(n)), page)
html.write_text(page)
