#!/usr/bin/env python3
"""Dump GENUINE GeometryList edge-table selection scans for a polygon at a given scale, via pdnhost
--seledges on the real-Windows VM. argv: <poly "x,y;x,y;..."> <scale>. Prints one per-row span
"y x0 x1" (sorted), to diff against the port's `classic_oracle selscans:<scale>:coords`.
Driven through subprocess argv (no shell) so ';' and spaces in the polygon survive intact."""
import subprocess, sys, pathlib

VMPY = "/home/alex/Documents/opend2d/harness/vm.py"
B = pathlib.Path(__file__).resolve().parents[1] / ".build" / "host-win9"
PDNRUN = r"C:\d2d-rs\pdn1\pdnrun"


def vm(*args):
    r = subprocess.run(["python3", VMPY, *args], capture_output=True, text=True)
    if r.returncode:
        sys.stderr.write(r.stderr)
        raise SystemExit(f"vm.py {args[0]} failed")
    return r.stdout


def main():
    poly, scale = sys.argv[1], sys.argv[2]
    # Stage host (idempotent); PDN's own DLLs are already copied by a prior run.
    for f in ("pdnhost.dll", "pdnhost.runtimeconfig.json"):
        vm("push", str(B / f), f"pdn1/pdnrun/{f}")
    exe = rf"{PDNRUN}\pdnhost.exe"
    cmd = (
        "Get-Process pdnhost -ErrorAction SilentlyContinue | Stop-Process -Force; "
        "Start-Sleep -Seconds 2; "
        f"cmd /c '{exe} --plugin {PDNRUN}\\PaintDotNet.Core.dll --asmdir {PDNRUN} "
        f"--seledges \"{poly}\" --scale {scale}'"
    )
    sys.stdout.write(vm("run", cmd))


if __name__ == "__main__":
    main()
