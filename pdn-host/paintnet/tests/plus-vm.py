#!/usr/bin/env python3
"""Render a GENUINE BoltBait 'Plus' plugin (whole image, square-tile schedule) on the real-Windows
VM via pdnhost --tiles, and pull the BGRA result — to byte-oracle the port's bb* composite.

argv: <effect-fullname> <plugin-dll-basename> <props-json> <local-in.bgra> <local-out.bgra> <w> <h>
The plugin DLL is taken from 080-int's boltbait/bin. Props go through a pushed file (--propsfile),
and pdnhost is invoked via PowerShell & with single-quoted args, to dodge cmd/PS quote mangling."""
import subprocess, sys, pathlib

VMPY = "/home/alex/Documents/opend2d/harness/vm.py"
HOST = pathlib.Path(__file__).resolve().parents[1] / ".build" / "host-win9"
PLUS = pathlib.Path("/home/alex/Documents/paint.rs/.worktrees/080-int/ref/pdn-plugins/boltbait/bin")
PDNRUN = r"C:\d2d-rs\pdn1\pdnrun"
WORK = r"C:\d2d-rs\pdn1\work"


def vm(*args):
    r = subprocess.run(["python3", VMPY, *args], capture_output=True, text=True)
    if r.returncode:
        sys.stderr.write(r.stderr)
        raise SystemExit(f"vm.py {args[0]} failed")
    return r.stdout


def main():
    effect, dll, props, lin, lout, w, h = sys.argv[1:8]
    for f in ("pdnhost.dll", "pdnhost.runtimeconfig.json"):
        vm("push", str(HOST / f), f"pdn1/pdnrun/{f}")
    vm("push", str(PLUS / dll), f"pdn1/work/{dll}")
    vm("push", lin, "pdn1/work/pin.bgra")
    pfile = pathlib.Path(lout).with_suffix(".props.json")
    pfile.write_text(props)
    vm("push", str(pfile), "pdn1/work/pprops.json")

    def q(s):
        return "'" + s.replace("'", "''") + "'"
    argv = [
        "--plugin", rf"{WORK}\{dll}", "--asmdir", PDNRUN, "--effect", effect, "--tiles",
        "--propsfile", rf"{WORK}\pprops.json",
        "--in", rf"{WORK}\pin.bgra", "--out", rf"{WORK}\pout.bgra", "--width", w, "--height", h,
    ]
    ps = (
        "Get-Process pdnhost -EA SilentlyContinue | Stop-Process -Force; Start-Sleep -Seconds 2; "
        "& '" + PDNRUN + "\\pdnhost.exe' " + " ".join(q(x) for x in argv)
    )
    sys.stdout.write(vm("run", ps)[-400:])
    vm("pull", "pdn1/work/pout.bgra", lout)


if __name__ == "__main__":
    main()
