#!/usr/bin/env python3
r"""End-to-end conformance runner for classic Paint.NET CPU plugins.

The point of comparison is byte-for-byte: our reflection host (pdnhost) renders
a plugin twice over the same input image and property values —

  * locally, loading our independent replacement assemblies, and
  * on the Windows VM, loading the genuine Paint.NET assemblies —

and the two raw-BGRA outputs must be identical.

Commands (run from anywhere; paths are resolved relative to this file):

  run.py build                 build shim, host, and every registered plugin
  run.py local  [--plugin P] [--size N]   render locally  -> artifacts/<P>.local.bgra
  run.py vm     [--plugin P] [--size N]   render on the VM -> artifacts/<P>.vm.bgra
  run.py compare [--plugin P]             byte-compare the two
  run.py all    [--plugin P] [--size N]   build + local + vm + compare

Executing a plugin runs third-party code; the dev sandbox may gate that.
"""
import argparse
import fcntl
import hashlib
import os
import subprocess
import sys

HOST = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))   # hosts/paintnet
REPO = os.path.dirname(os.path.dirname(HOST))                        # repo root
BUILD = os.path.join(HOST, ".build")
ART = os.path.join(HOST, "artifacts")
DN = os.path.join(HOST, "dn.sh")
VMPY = os.path.join(REPO, "harness", "vm.py")
PDN_REF = "/home/alex/Documents/opend2d/ref/pdn-wine"   # genuine assemblies (read-only, untracked)
VM_BASE = r"pdn1"                                      # our folder under C:\d2d-rs
VM_LOCK = "/tmp/d2d-vm.lock"
BUILD_FLAGS = ["-nologo", "-v", "q", "-m:1", "--disable-build-servers", "-nodeReuse:false"]

# name -> (csproj relative to HOST, built dll name)
PLUGINS = {
    "mirror": ("../../ref/pdn-plugins/mirror/Mirror.csproj", "Mirror.dll"),
}


def dn(*args):
    subprocess.run([DN, *args], check=True, cwd=HOST)


def gen_image(path, size):
    """Deterministic BGRA test image (same bytes drive both runs)."""
    buf = bytearray()
    for y in range(size):
        for x in range(size):
            buf += bytes([(x * 4) & 255, (y * 4) & 255, ((x + y) * 2) & 255, 255])
    with open(path, "wb") as f:
        f.write(buf)


def build(_args):
    # One unified assembly set (assemblies/). The Effects and Base forwarders pull in
    # Effects.Core, Effects.Legacy, Data, Core and PropertySystem; WindowsForms is the
    # headless System.Windows.Forms stand-in.
    for proj in ("Effects/Effects.csproj", "Base/Base.csproj", "WindowsForms/WindowsForms.csproj"):
        dn("build", f"assemblies/{proj}", "-o", f"{BUILD}/shim", *BUILD_FLAGS)
    dn("build", "host/pdnhost.csproj", "-o", f"{BUILD}/host", *BUILD_FLAGS)
    # Stage the GDI+ bits (System.Drawing.Common 6.0 unix + libgdiplus + the System.Drawing facade)
    # into the shim so third-party plugins that touch System.Drawing resolve them in-context.
    subprocess.check_call([os.path.join(HOST, "tests", "stage-gdiplus.sh")])
    for name, (csproj, _dll) in PLUGINS.items():
        dn("build", csproj, "-o", f"{BUILD}/plugin-{name}", *BUILD_FLAGS)


def local(args):
    os.makedirs(ART, exist_ok=True)
    _dll = PLUGINS[args.plugin][1]
    img = os.path.join(ART, f"{args.plugin}.in.bgra")
    out = os.path.join(ART, f"{args.plugin}.local.bgra")
    gen_image(img, args.size)
    dn("exec", f"{BUILD}/host/pdnhost.dll",
       "--plugin", f"{BUILD}/plugin-{args.plugin}/{_dll}",
       "--asmdir", f"{BUILD}/shim",
       "--in", img, "--out", out,
       "--width", str(args.size), "--height", str(args.size))
    print("local ->", out, _sha(out))


def selftest(args):
    """Render an in-tree Invert effect through the host+shim and check the pixels.
    No third-party plugin, no VM: a self-contained regression check of the pipeline."""
    dn("build", "tests/selftest/selftest.csproj", "-o", f"{BUILD}/selftest", *BUILD_FLAGS)
    os.makedirs(ART, exist_ok=True)
    img = os.path.join(ART, "selftest.in.bgra")
    out = os.path.join(ART, "selftest.out.bgra")
    gen_image(img, args.size)
    dn("exec", f"{BUILD}/host/pdnhost.dll", "--plugin", f"{BUILD}/selftest/PdnSelfTest.dll",
       "--effect", "PdnSelfTest.InvertEffect",
       "--asmdir", f"{BUILD}/shim", "--in", img, "--out", out,
       "--width", str(args.size), "--height", str(args.size))
    a, b = open(img, "rb").read(), open(out, "rb").read()
    ok = len(a) == len(b) > 0 and all(
        b[i] == 255 - a[i] and b[i + 1] == 255 - a[i + 1] and b[i + 2] == 255 - a[i + 2] and b[i + 3] == a[i + 3]
        for i in range(0, len(a), 4))
    print("selftest", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def _vm(*argv):
    subprocess.run([sys.executable, VMPY, *argv], check=True, cwd=REPO)


def vm(args):
    os.makedirs(ART, exist_ok=True)
    name = args.plugin
    dll = PLUGINS[name][1]
    img = os.path.join(ART, f"{name}.in.bgra")
    gen_image(img, args.size)
    # Self-contained host so the VM needs no .NET install.
    dn("publish", "host/pdnhost.csproj", "-c", "Release", "-r", "win-x64",
       "--self-contained", "true", "-o", f"{BUILD}/host-win", *BUILD_FLAGS)

    with open(VM_LOCK, "w") as lk:
        fcntl.flock(lk, fcntl.LOCK_EX)
        try:
            # Genuine assemblies: every managed/native PaintDotNet DLL + deps.
            for fn in sorted(os.listdir(PDN_REF)):
                if fn.startswith("PaintDotNet.") and fn.endswith(".dll") or fn == "CommunityToolkit.HighPerformance.dll":
                    _vm("push", os.path.join(PDN_REF, fn), rf"{VM_BASE}\pdnref\{fn}")
            for fn in sorted(os.listdir(f"{BUILD}/host-win")):
                _vm("push", os.path.join(BUILD, "host-win", fn), rf"{VM_BASE}\host\{fn}")
            _vm("push", f"{BUILD}/plugin-{name}/{dll}", rf"{VM_BASE}\work\{dll}")
            _vm("push", img, rf"{VM_BASE}\work\{name}.in.bgra")
            script = (
                rf"& C:\d2d-rs\{VM_BASE}\host\pdnhost.exe "
                rf"--plugin C:\d2d-rs\{VM_BASE}\work\{dll} "
                rf"--asmdir C:\d2d-rs\{VM_BASE}\pdnref "
                rf"--in C:\d2d-rs\{VM_BASE}\work\{name}.in.bgra "
                rf"--out C:\d2d-rs\{VM_BASE}\work\{name}.vm.bgra "
                rf"--width {args.size} --height {args.size}"
            )
            _vm("run", script)
            _vm("pull", rf"{VM_BASE}\work\{name}.vm.bgra", os.path.join(ART, f"{name}.vm.bgra"))
        finally:
            fcntl.flock(lk, fcntl.LOCK_UN)
    print("vm ->", os.path.join(ART, f"{name}.vm.bgra"), _sha(os.path.join(ART, f"{name}.vm.bgra")))


def compare(args):
    a = os.path.join(ART, f"{args.plugin}.local.bgra")
    b = os.path.join(ART, f"{args.plugin}.vm.bgra")
    da, db = open(a, "rb").read(), open(b, "rb").read()
    if da == db:
        print(f"IDENTICAL  {args.plugin}  {_sha(a)}  ({len(da)} bytes)")
        return 0
    first = next((i for i in range(min(len(da), len(db))) if da[i] != db[i]), min(len(da), len(db)))
    print(f"DIFFER     {args.plugin}  local={_sha(a)} vm={_sha(b)}  "
          f"len {len(da)}/{len(db)}  first diff at byte {first}")
    return 1


def _sha(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()[:16]


def all_cmd(args):
    build(args)
    local(args)
    vm(args)
    return compare(args)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    for cmd in ("build", "selftest", "local", "vm", "compare", "all"):
        sp = sub.add_parser(cmd)
        if cmd not in ("build",):
            sp.add_argument("--size", type=int, default=64)
        if cmd not in ("build", "selftest"):
            sp.add_argument("--plugin", default="mirror", choices=list(PLUGINS))
    args = p.parse_args()
    fn = {"build": build, "selftest": selftest, "local": local, "vm": vm, "compare": compare, "all": all_cmd}[args.cmd]
    sys.exit(fn(args) or 0)


if __name__ == "__main__":
    main()
