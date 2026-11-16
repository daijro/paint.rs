#!/usr/bin/env python3
r"""Extract and classify Paint.NET's built-in GPU effect pixel shaders from
ref/pdn-wine/PaintDotNet.Effects.Gpu.dll (ComputeSharp.D2D1 DXBC).

The DLL is gitignored reference material; its DXBC bytecode is never committed.
This writes each shader's DXBC into OUT_DIR (a gitignored build dir) and prints a
classification manifest (effect, shader, input count, simple/complex sampling,
resource textures, scene-position, cbuffer layout) to stdout as JSON.

  python3 extract.py GPU_DLL GPU_CS OUT_DIR > manifest.json

GPU_CS is `ilspycmd GPU_DLL` output (for the HLSL sources that say which inputs are
D2DInputComplex and which resource textures a shader binds -- the facts that decide
whether crates/d2d's effects/shader.rs can run a shader 1:1). Pass "-" to skip HLSL
classification (DXBC + cbuffer layout only)."""
import sys, re, struct, hashlib, json, os

def dxbc_blobs(data):
    out, i = [], 0
    while True:
        j = data.find(b'DXBC', i)
        if j < 0 or j + 32 > len(data): break
        total, nch = struct.unpack_from('<II', data, j + 24)
        if 0 < total < 300000 and 0 < nch < 16 and j + total <= len(data):
            offs = struct.unpack_from('<%dI' % nch, data, j + 32)
            if all(o + 8 <= total for o in offs):
                out.append(data[j:j + total]); i = j + total; continue
        i = j + 4
    return out

def chunks(b):
    nch = struct.unpack_from('<I', b, 28)[0]
    offs = struct.unpack_from('<%dI' % nch, b, 32)
    return {b[o:o + 4].decode('latin1'): (o + 8, struct.unpack_from('<I', b, o + 4)[0]) for o in offs}

def cstr(b, o):
    return b[o:b.index(b'\0', o)].decode('latin1')

def blob_info(b):
    ch = chunks(b)
    base = ch['RDEF'][0]
    ncb, cboff, nres, resoff, ver = struct.unpack_from('<IIIII', b, base)
    glob, cbsize = {}, 0
    for k in range(ncb):
        no, nvar, voff, size = struct.unpack_from('<IIII', b, base + cboff + k * 24)[:4]
        if cstr(b, base + no) == '$Globals':
            cbsize = size
            for vi in range(nvar):
                vno, vso, vsz = struct.unpack_from('<III', b, base + voff + vi * 40)[:3]
                glob[cstr(b, base + vno)] = (vso, vsz)
    ntex = sum(1 for k in range(nres)
               if struct.unpack_from('<II', b, base + resoff + k * 32)[1] == 2)
    tex = 0
    if 'ISGN' in ch:
        ib = ch['ISGN'][0]; n = struct.unpack_from('<I', b, ib)[0]
        for k in range(n):
            no = struct.unpack_from('<I', b, ib + 8 + k * 24)[0]
            if cstr(b, ib + no) == 'TEXCOORD': tex += 1
    return dict(cbsize=cbsize, globals=glob, ntex=ntex, ic=tex, sha=hashlib.sha256(b).hexdigest())

def hlsl_records(cs):
    """Per HlslSource (one per shader pass, all 50): shader struct name, input
    count, complex/simple sampling, scene-position, #resource textures, and the
    __ConstantBuffer user-constant field names that pin it to its DXBC blob."""
    cbs = {}  # shader struct name -> list of field sets (declaration order)
    for m in re.finditer(r'struct <PaintDotNet_Effects_Gpu_(Pdn\w+?)(?:_Impl)?_(\w+)_g>\w+__ConstantBuffer\b', cs):
        body = cs[m.end():cs.index('}', m.end())]
        fields = re.findall(r'(?:public |internal )?(?:float[1-4]?|uint[1-4]?|int[1-4]?|bool)\s+(\w+)\s*;', body)
        cbs.setdefault(m.group(2), []).append((m.group(1), set(fields)))
    srcs, seen = [], {}
    for m in re.finditer(r'ID2D1PixelShaderDescriptor<(\w+)>\.HlslSource => "((?:[^"\\]|\\.)*)";', cs):
        sh = m.group(1)
        src = m.group(2).encode('latin1').decode('unicode_escape')
        ic = int(re.search(r'#define D2D_INPUT_COUNT (\d+)', src).group(1))
        complex_ = ('_COMPLEX' in src) or ('D2DSampleInputAtOffset' in src) or ('D2DSampleInputAtPosition' in src)
        scene = 'D2DGetScenePosition' in src
        texregs = re.findall(r'Texture\dD<[^>]*>\s+\w+\s*:\s*register\(t(\d+)\)', src)
        k = seen.get(sh, 0); seen[sh] = k + 1
        eff, fields = cbs.get(sh, [('?', set())] * (k + 1))[k] if k < len(cbs.get(sh, [])) else ('?', set())
        srcs.append(dict(effect=eff, shader=sh, ic=ic, complex=complex_, scene=scene,
                         n_restex=len(texregs), fields=set(fields)))
    return srcs

def main():
    dll, cspath, outdir = sys.argv[1], sys.argv[2], sys.argv[3]
    blobs = [(b, blob_info(b)) for b in dxbc_blobs(open(dll, 'rb').read())]
    cs = open(cspath, encoding='utf-8', errors='replace').read()
    srcs = hlsl_records(cs)
    # Match each shader pass to its DXBC blob by its user-constant field set: the blob whose $Globals
    # is a superset with the fewest *extra* globals (ComputeSharp injects system constants such as
    # `size`/`scenePosOffset`), 1:1. Field sets are unique per shader, so this is exact; passes with
    # no user constants (e.g. a bare combine shader) stay unmatched (blob = null).
    used = set()
    for r in srcs:
        f = r['fields']
        cand = sorted((len(set(inf['globals']) - f), bi)
                      for bi, (b, inf) in enumerate(blobs)
                      if f and bi not in used and f <= set(inf['globals']))
        r['blob'] = cand[0][1] if cand and (len(cand) == 1 or cand[0][0] < cand[1][0]) else None
        if r['blob'] is not None:
            used.add(r['blob'])
    os.makedirs(outdir, exist_ok=True)
    manifest = []
    for r in srcs:
        bi = r['blob']
        inf = blobs[bi][1] if bi is not None else {}
        name = f"{r['effect']}_{r['shader']}"
        if bi is not None:
            open(os.path.join(outdir, name + '.dxbc'), 'wb').write(blobs[bi][0])
        manifest.append(dict(name=name, effect=r['effect'], shader=r['shader'],
                             ic=r['ic'], complex=r['complex'], res_tex=r['n_restex'],
                             scene=r['scene'], cbsize=inf.get('cbsize'),
                             sha=inf.get('sha'), blob=bi,
                             runnable=(not r['complex']) and r['n_restex'] == 0))
    print(json.dumps(manifest, indent=1))

main()
