#!/usr/bin/env python3
"""Trunk's post_build hook (Trunk.toml): the MobileSAM weights for the web's worker, split into parts of
at most 20 MiB (Cloudflare Pages takes files of up to 25 MiB), with their licence:
<staging>/models/mobilesam/mobile_sam-tiny-vitt-<sha8>.safetensors.0, .1, … and LICENSE, <sha8> being
the first 8 hex digits of the whole file's SHA-256, which versions the parts. The worker
(crates/sam-worker) fetches and joins them until it has the whole file, then checks its size and SHA-256.
Only this script knows the part size."""
import hashlib
import os
from pathlib import Path

PART = 20 << 20
source = Path(os.environ['TRUNK_SOURCE_DIR']) / 'assets/models/mobilesam'
out = Path(os.environ['TRUNK_STAGING_DIR']) / 'models/mobilesam'
out.mkdir(parents=True, exist_ok=True)
data = (source / 'mobile_sam-tiny-vitt.safetensors').read_bytes()
name = f'mobile_sam-tiny-vitt-{hashlib.sha256(data).hexdigest()[:8]}.safetensors'
for i in range(0, len(data), PART):
    (out / f'{name}.{i // PART}').write_bytes(data[i:i + PART])
(out / 'LICENSE').write_bytes((source / 'LICENSE').read_bytes())
