#!/usr/bin/env python3
"""Extract Paint.NET's *full* ps_5_0 pixel-shader corpus from the reference DLLs for the coverage meter.

Where extract.py classifies one assembly's ComputeSharp effects, this scans every PDN assembly's raw
bytes for DXBC containers and keeps only standalone ps_5_0 PIXEL programs (program type 0 — excludes
the nested lib_5_0 / 0xfff0 link-function containers that live in each shader's PRIV chunk), dedups by
content, and writes `<sha256[:16]>.dxbc`. That 16-hex stem is the name opend2d's `shader-ref --batch`
and `crates/d2d-shader/tests/pdn_coverage.rs` key captures by, so re-running this reproduces the exact
corpus the meter ran against (711 distinct across six assemblies).

The shader bytes are Microsoft/Paint.NET property and are never committed; this regenerates them from
the gitignored read-only reference DLLs. Usage: `extract-corpus.py REF_DIR OUT_DIR`
(REF_DIR = the ref/pdn-wine directory of the opend2d checkout).
"""
import struct, hashlib, os, sys

# The six PDN assemblies that embed pixel programs (the two d3dcompiler DLLs carry compiler blobs, not
# effects, so they are excluded). 718 candidate containers -> 711 distinct ps_5_0 pixel shaders.
ASSEMBLIES = [
    "PaintDotNet.Windows.Framework.dll",
    "PaintDotNet.Windows.Direct2D1.Managed.dll",
    "PaintDotNet.Effects.Gpu.dll",
    "PaintDotNet.Framework.dll",
    "paintdotnet.dll",
    "PaintDotNet.Core.dll",
]


def chunks(blob):
    """Yield (tag, data) for each DXBC chunk. Header: magic(4) cksum(16) ver(4) total(4) nchunks(4)."""
    n = struct.unpack_from("<I", blob, 28)[0]
    for o in struct.unpack_from(f"<{n}I", blob, 32):
        if o + 8 > len(blob):
            return
        sz = struct.unpack_from("<I", blob, o + 4)[0]
        yield blob[o:o + 4], blob[o + 8:o + 8 + sz]


def is_ps50(blob):
    """True iff the program chunk is a pixel shader 5.0 (version token: type<<16 | major<<4 | minor)."""
    for tag, data in chunks(blob):
        if tag in (b"SHEX", b"SHDR") and len(data) >= 4:
            v = struct.unpack_from("<I", data, 0)[0]
            return (v >> 16) & 0xFFFF == 0 and (v >> 4) & 0xF == 5 and v & 0xF == 0
    return False


def main():
    ref, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    for f in os.listdir(out):
        if f.endswith(".dxbc"):
            os.remove(os.path.join(out, f))
    seen, kept, candidates = set(), 0, 0
    for name in ASSEMBLIES:
        data = open(os.path.join(ref, name), "rb").read()
        i = 0
        while (i := data.find(b"DXBC", i)) >= 0:
            if i + 32 > len(data):
                break
            ver, tsz, nch = struct.unpack_from("<III", data, i + 20)
            if ver != 1 or not (32 <= tsz <= 4_000_000) or i + tsz > len(data) or not (1 <= nch <= 64):
                i += 4
                continue
            blob = data[i:i + tsz]
            candidates += 1
            try:
                ok = is_ps50(blob)
            except Exception:
                ok = False
            if ok:
                h = hashlib.sha256(blob).hexdigest()
                if h not in seen:
                    seen.add(h)
                    open(os.path.join(out, f"{h[:16]}.dxbc"), "wb").write(blob)
                    kept += 1
            i += tsz  # skip the whole container; its PRIV-nested DXBC lives inside this span
    print(f"{candidates} candidate DXBC containers scanned; {kept} distinct ps_5_0 pixel shaders -> {out}")


if __name__ == "__main__":
    main()
