#!/usr/bin/env python3
"""Generate the Direct2D/DirectWrite COM call layer the replacement PaintDotNet.Windows uses over opend2d.

Slot order, signatures, IIDs, effect CLSIDs and by-value structs all come from opend2d's
crates/windows-shim/src/bindings.rs (the windows-bindgen output its d2d-ffi/dwrite-ffi COM objects
implement), so nothing is transcribed by hand. Each interface becomes a static class whose methods call
the vtable slot through an unmanaged function pointer:

    ID2D1Effect.SetInput(p, index, image, invalidate)   // slot 14 of ID2D1Effect_Vtbl

Usage: gen_com.py <opend2d>/crates/windows-shim/src/bindings.rs > assemblies/Windows/Interop/Com.g.cs
"""
import re
import sys

# The interfaces the managed wrappers call; their base interfaces are emitted too.
WANTED = """
ID2D1Factory1 ID2D1Device ID2D1DeviceContext ID2D1Bitmap1 ID2D1Effect ID2D1CommandList
ID2D1SolidColorBrush ID2D1LinearGradientBrush ID2D1RadialGradientBrush ID2D1ImageBrush ID2D1BitmapBrush1
ID2D1GradientStopCollection ID2D1RectangleGeometry ID2D1EllipseGeometry ID2D1RoundedRectangleGeometry
ID2D1TransformedGeometry ID2D1PathGeometry ID2D1GeometryGroup ID2D1GeometrySink ID2D1StrokeStyle1
ID2D1ColorContext ID2D1Layer ID2D1DeviceContext2 ID2D1DeviceContext5
IDWriteFactory IDWriteTextFormat IDWriteTextLayout IDWriteFontCollection IDWriteFontFamily IDWriteFont
IDWriteLocalizedStrings IDWriteFontList IDWriteGdiInterop IDWriteFontFace
""".split()

PRIM = {"u8": "byte", "i8": "sbyte", "u16": "ushort", "i16": "short", "u32": "uint", "i32": "int",
        "u64": "ulong", "i64": "long", "f32": "float", "f64": "double", "usize": "nuint", "isize": "nint",
        "BOOL": "int", "HRESULT": "int", "PCWSTR": "char*", "PWSTR": "char*", "PCSTR": "byte*",
        "GUID": "Guid", "c_void": "void", "HANDLE": "nint", "HWND": "nint", "HDC": "nint", "HMONITOR": "nint",
        "BOOLEAN": "byte", "FILETIME": "ulong", "LUID": "ulong"}
# windows-numerics value types (not in bindings.rs).
NUMERICS = {"Vector2": ["float X", "float Y"], "Vector3": ["float X", "float Y", "float Z"],
            "Vector4": ["float X", "float Y", "float Z", "float W"],
            "Matrix3x2": ["float M11", "float M12", "float M21", "float M22", "float M31", "float M32"],
            "Matrix4x4": [f"float M{r}{c}" for r in range(1, 5) for c in range(1, 5)]}


def split_top(s, sep=","):
    """Split on `sep` outside (), [], <>."""
    out, depth, cur = [], 0, ""
    for i, ch in enumerate(s):
        if ch in "([<":
            depth += 1
        elif ch in ")]>" and not (ch == ">" and s[i - 1] == "-"):  # `->` is not a bracket
            depth -= 1
        if ch == sep and depth == 0:
            out.append(cur)
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur)
    return [x.strip() for x in out if x.strip()]


def block(src, start):
    """The text between the `{` at/after `start` and its matching `}`."""
    i = src.index("{", start)
    depth = 0
    for j in range(i, len(src)):
        if src[j] == "{":
            depth += 1
        elif src[j] == "}":
            depth -= 1
            if depth == 0:
                return src[i + 1:j]
    raise ValueError("unbalanced")


src = open(sys.argv[1]).read()
newtypes = {m.group(1): m.group(2) for m in re.finditer(r"pub struct (\w+)\(pub (\w+)\);", src)}
aliases = {m.group(1): m.group(2) for m in re.finditer(r"pub type (\w+) =\s*([^;]+);", src)}
structs = {}
for m in re.finditer(r"pub struct (\w+) \{", src):
    if m.group(1).endswith("_Vtbl"):
        continue
    body = re.sub(r"\s+", " ", block(src, m.end() - 1))
    fields = [f for f in split_top(body) if f.startswith("pub ")]
    structs.setdefault(m.group(1), [tuple(x.strip() for x in f[4:].split(":", 1)) for f in fields])
vtbls = {}
for m in re.finditer(r"pub struct (\w+)_Vtbl \{", src):
    body = re.sub(r"\s+", " ", block(src, m.end() - 1))
    vtbls[m.group(1)] = split_top(body)
iids = {m.group(1): m.group(2) for m in re.finditer(
    r"define_interface!\(\s*(\w+),\s*\w+_Vtbl,\s*0x([0-9a-f_]+)\s*\)", src)}
clsids = sorted(set(re.findall(
    r"pub const (CLSID_D2D1\w+): windows_core::GUID =\s*windows_core::GUID::from_u128\(0x([0-9a-f_]+)\);", src)))

needed_structs = []


def last(t):
    return re.sub(r"^.*::", "", t.strip())


def cs(t):
    t = t.strip()
    if t.startswith("Option<"):  # a nullable callback
        return "void*"
    if t.startswith("*"):
        inner = re.sub(r"^\*(mut|const) ", "", t)
        return "void**" if inner.startswith("*") else "void*"
    if t.startswith("["):  # fixed array by value: never in a vtable signature
        raise ValueError(t)
    n = last(t)
    if n in PRIM:
        return PRIM[n]
    if n in aliases:
        return cs(aliases[n])
    if n in newtypes:
        return PRIM.get(newtypes[n], newtypes[n])
    if n in NUMERICS or n in structs:
        if n not in needed_structs:
            needed_structs.append(n)
            for _, ft in structs.get(n, []):
                if not ft.startswith(("*", "[")):
                    cs(ft)
        return n
    raise ValueError(f"unmapped type {t}")


def guid(h):
    h = h.replace("_", "")
    return f"{h[0:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:32]}"


def methods(iface):
    """[(slot, name, fields)] own methods of iface, and the slot count of its base chain."""
    fields = vtbls[iface]
    base_field = fields[0]
    base = last(base_field.split(":", 1)[1]).removesuffix("_Vtbl")
    base_slots = 3 if base == "IUnknown" else slots(base)
    out = []
    for i, f in enumerate(fields[1:]):
        name, typ = (x.strip() for x in f.split(":", 1))
        out.append((base_slots + i, name.removeprefix("pub "), typ))
    return base, base_slots, out


def slots(iface):
    base, n, own = methods(iface)
    return n + len(own)


order = []


def want(iface):
    if iface in order or iface == "IUnknown":
        return
    base, _, _ = methods(iface)
    want(base)
    order.append(iface)


for w in WANTED:
    want(w)

lines = []
for iface in order:
    base, _, own = methods(iface)
    lines.append(f"    /// <summary><c>{iface}</c> : <c>{base}</c>.</summary>")
    lines.append(f"    internal static unsafe class {iface}")
    lines.append("    {")
    if iface in iids:
        lines.append(f"        public static readonly Guid IID = new(\"{guid(iids[iface])}\");")
    for slot, name, typ in own:
        if typ == "usize":
            lines.append(f"        // slot {slot}: {name} (not bound)")
            continue
        m = re.match(r"unsafe extern \"system\" fn\((.*)\)\s*(?:->\s*(.+))?$", typ)
        params = [cs(p) for p in split_top(m.group(1))]
        ret = cs(m.group(2)) if m.group(2) else "void"
        args = params[1:]
        decl = ", ".join(["void* p"] + [f"{t} a{i}" for i, t in enumerate(args)])
        call = ", ".join(["p"] + [f"a{i}" for i in range(len(args))])
        fp = ", ".join(params + [ret])
        lines.append(f"        public static {ret} {name}({decl}) => "
                     f"((delegate* unmanaged<{fp}>)(*(void***)p)[{slot}])({call});")
    lines.append("    }")
    lines.append("")

print("// <auto-generated> by pdn-host/paintnet/tools/gen_com.py from opend2d's")
print("// crates/windows-shim/src/bindings.rs: vtable slots, IIDs and effect CLSIDs. Do not edit.")
print("#pragma warning disable CS0108, CS0649, IDE1006")
print("using System;")
print()
print("namespace PaintDotNet.Interop")
print("{")
print("    internal static unsafe class IUnknown")
print("    {")
print("        public static readonly Guid IID = new(\"00000000-0000-0000-c000-000000000046\");")
print("        public static int QueryInterface(void* p, Guid* iid, void** o) => "
      "((delegate* unmanaged<void*, Guid*, void**, int>)(*(void***)p)[0])(p, iid, o);")
print("        public static uint AddRef(void* p) => ((delegate* unmanaged<void*, uint>)(*(void***)p)[1])(p);")
print("        public static uint Release(void* p) => ((delegate* unmanaged<void*, uint>)(*(void***)p)[2])(p);")
print("    }")
print()
print("\n".join(lines))
print("    internal static class Clsid")
print("    {")
for name, h in clsids:
    print(f"        public static readonly Guid {name.removeprefix('CLSID_')} = new(\"{guid(h)}\");")
print("    }")
print()
for n in needed_structs:
    fs = NUMERICS.get(n) or [f"{cs(t) if not t.startswith('[') else 'fixed'} {f}" for f, t in structs[n]]
    print(f"    internal struct {n} {{ {' '.join('public ' + f + ';' for f in fs)} }}")
print("}")
