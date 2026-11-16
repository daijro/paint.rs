#!/usr/bin/env sh
# Compile scale-sample.hlsl -> scale-sample.dxbc, once, and commit the result. Linux has
# no fxc, so we drive D3DCompile from Microsoft's d3dcompiler_47.dll under Wine through a
# tiny cross-compiled stub. This keeps the sample shader paint.rs's own: its source is
# scale-sample.hlsl here, and the bytecode is generated from that file — no dependency on
# opend2d's (private, unshippable) conformance suite.
#
# Prereqs: x86_64-w64-mingw32-gcc, wine, and a d3dcompiler_47.dll (point D3DCOMPILER_DIR at
# a folder holding it; defaults to Paint.NET's Wine runtime under opend2d/ref, dev-only).
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
d3ddir="${D3DCOMPILER_DIR:-/home/alex/Documents/opend2d/ref/pdn-wine}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cp "$d3ddir/d3dcompiler_47.dll" "$work/"

# Stub: compile argv[1] (HLSL) at ps_4_0 and write the DXBC blob to argv[2].
cat > "$work/fxc_stub.c" <<'C'
#include <windows.h>
#include <d3dcompiler.h>
#include <stdio.h>
int main(int argc, char **argv) {
    if (argc != 3) { fprintf(stderr, "usage: fxc_stub <in.hlsl> <out.dxbc>\n"); return 2; }
    FILE *f = fopen(argv[1], "rb"); if (!f) { perror("open hlsl"); return 1; }
    fseek(f, 0, SEEK_END); long n = ftell(f); fseek(f, 0, SEEK_SET);
    char *src = malloc(n); fread(src, 1, n, f); fclose(f);
    ID3DBlob *code = NULL, *errs = NULL;
    HRESULT hr = D3DCompile(src, n, argv[1], NULL, NULL, "main", "ps_4_0",
                            D3DCOMPILE_OPTIMIZATION_LEVEL3, 0, &code, &errs);
    if (errs) fprintf(stderr, "%.*s\n", (int)errs->lpVtbl->GetBufferSize(errs),
                      (char *)errs->lpVtbl->GetBufferPointer(errs));
    if (FAILED(hr)) { fprintf(stderr, "D3DCompile failed: 0x%08lx\n", hr); return 1; }
    FILE *o = fopen(argv[2], "wb"); if (!o) { perror("open out"); return 1; }
    fwrite(code->lpVtbl->GetBufferPointer(code), 1, code->lpVtbl->GetBufferSize(code), o);
    fclose(o);
    return 0;
}
C
x86_64-w64-mingw32-gcc -O2 "$work/fxc_stub.c" -o "$work/fxc_stub.exe" -ld3dcompiler
cp "$here/scale-sample.hlsl" "$work/"
( cd "$work" && WINEDEBUG=-all wine fxc_stub.exe scale-sample.hlsl scale-sample.dxbc )
cp "$work/scale-sample.dxbc" "$here/scale-sample.dxbc"
echo "compiled -> $here/scale-sample.dxbc ($(wc -c < "$here/scale-sample.dxbc") bytes)"
