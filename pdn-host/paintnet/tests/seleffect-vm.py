#!/usr/bin/env python3
"""Render a GENUINE Paint.NET classic effect UNDER a polygon selection on the real-Windows VM, via
pdnhost --tiles --selpoly, and pull the BGRA result — to byte-oracle the port's selection path
(mask -> coverage scans -> square tiles -> ROI-dependent render -> underwrite).

argv: <effect-fullname> <poly "x,y;x,y;..."> <props-json> <local-in.bgra> <local-out.bgra> <w> <h>
      [--plugin <dll-basename>]   (default PaintDotNet.Effects.Legacy.dll)

Driven through subprocess argv, and the pdnhost call uses PowerShell's & operator with single-quoted
args, so ';' in the polygon and the quotes in the props JSON survive intact (cmd /c eats them)."""
import subprocess, sys, pathlib

VMPY = "/home/alex/Documents/opend2d/harness/vm.py"
B = pathlib.Path(__file__).resolve().parents[1] / ".build" / "host-win9"
PDNRUN = r"C:\d2d-rs\pdn1\pdnrun"
WORK = r"C:\d2d-rs\pdn1\work"


def vm(*args):
    r = subprocess.run(["python3", VMPY, *args], capture_output=True, text=True)
    if r.returncode:
        sys.stderr.write(r.stderr)
        raise SystemExit(f"vm.py {args[0]} failed")
    return r.stdout


def main():
    effect, poly, props, lin, lout, w, h = sys.argv[1:8]
    plugin = "PaintDotNet.Effects.Legacy.dll"
    if "--plugin" in sys.argv:
        plugin = sys.argv[sys.argv.index("--plugin") + 1]

    for f in ("pdnhost.dll", "pdnhost.runtimeconfig.json"):
        vm("push", str(B / f), f"pdn1/pdnrun/{f}")
    vm("push", lin, "pdn1/work/sin.bgra")
    # Props JSON goes through a pushed file, not the command line, to dodge PowerShell/cmd quote eating.
    pfile = pathlib.Path(lout).with_suffix(".props.json")
    pfile.write_text(props)
    vm("push", str(pfile), "pdn1/work/sprops.json")

    # PowerShell & with single-quoted args: each is one literal argv, ';' in the polygon preserved.
    def q(s):
        return "'" + s.replace("'", "''") + "'"
    argv = [
        "--plugin", rf"{WORK}\{plugin}", "--asmdir", PDNRUN, "--effect", effect,
        "--tiles", "--selpoly", poly, "--propsfile", rf"{WORK}\sprops.json",
        "--in", rf"{WORK}\sin.bgra", "--out", rf"{WORK}\sout.bgra", "--width", w, "--height", h,
    ]
    ps = (
        "Copy-Item 'C:\\Program Files\\paint.net\\" + plugin + "' " + WORK + " -Force; "
        "Get-Process pdnhost -EA SilentlyContinue | Stop-Process -Force; Start-Sleep -Seconds 2; "
        "& '" + PDNRUN + "\\pdnhost.exe' " + " ".join(q(x) for x in argv)
    )
    out = vm("run", ps)
    sys.stdout.write(out[-400:])
    vm("pull", "pdn1/work/sout.bgra", lout)


if __name__ == "__main__":
    main()
