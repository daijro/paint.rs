#!/usr/bin/env python3
r"""Capture the WARP oracle for the Paint.NET built-in GPU effect shaders listed in effects.json.

For each capture entry it builds a `register_shader_effect` scene -- the extracted (gitignored) DXBC
inline, the entry's constant-buffer floats at b0, and the shared 32x32 gradient as the single input --
then drives the Windows `d2d-ref.exe` on the reference VM's WARP device (a real ID2D1Effect running
that DXBC, the same oracle fxshader-01 uses) and pulls `<scene>.bgra`/`.json` into
conformance/oracle/win11-26100/. Each shader is captured twice and the two `.bgra` must be identical.

The scene JSON embeds Paint.NET DXBC, so it is written only to a gitignored dir, never committed; the
raw-pixel captures are committable oracle output. Run the VM phase under `flock /tmp/d2d-vm.lock` with
`allowed_domains:["192.168.122.207:9334"]`; each d2d-ref run is a 32x32 render (sub-second).

  python3 capture.py EFFECTS_JSON DXBC_DIR SCENE_OUT ORACLE_OUT [--build-only]
"""
import sys, os, json, hashlib

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))  # builtin->tests->paintnet-gpu->hosts->repo
sys.path.insert(0, os.path.join(REPO, "harness"))  # harness/vm.py

def gradient():
    px = []
    for y in range(32):
        for x in range(32):
            px += [min(8 * x, 255), min(8 * y, 255), min(4 * (x + y), 255), 255]
    return px

def build_scene(entry, dxbc):
    clsid = "7e3a9f10-2b4c-4d5e-8f60-1a2b3c4d5e6f"
    return {"version": 2, "size": [32, 32], "dpi": 96, "ops": [
        {"op": "clear", "color": [0, 0, 0, 0]},
        {"op": "register_shader_effect", "clsid": clsid, "dxbc": list(dxbc),
         "cbuffer": entry["cbuffer"], "inputs": 1, "filter": 0},
        {"op": "bitmap", "id": "src", "size": [32, 32], "rgba": gradient()},
        {"op": "effect", "id": "fx", "clsid": clsid, "inputs": ["src"], "properties": []},
        {"op": "draw_image", "image": "fx", "offset": [0, 0], "interpolation": 1, "composite": 0},
    ]}

def main():
    effects_json, dxbc_dir, scene_out, oracle_out = sys.argv[1:5]
    build_only = "--build-only" in sys.argv
    manifest = json.load(open(effects_json))
    os.makedirs(scene_out, exist_ok=True)
    os.makedirs(oracle_out, exist_ok=True)
    entries = []
    for c in manifest["capture"]:
        dxbc_path = os.path.join(dxbc_dir, c["name"] + ".dxbc")
        dxbc = open(dxbc_path, "rb").read()
        assert hashlib.sha256(dxbc).hexdigest() == c["sha256"], f"DXBC sha mismatch for {c['name']}"
        scene = build_scene(c, dxbc)
        sp = os.path.join(scene_out, c["scene"] + ".json")
        json.dump(scene, open(sp, "w"))
        entries.append((c["scene"], sp))
        print(f"built {sp} ({len(dxbc)} B dxbc)")
    if build_only:
        return

    # VM phase: push d2d-ref + scenes to C:\d2d-rs\pdngpu2, run each scene twice (d2d-ref writes a
    # sibling .bgra/.json next to the OUT.png), pull both runs, require the two .bgra to be identical,
    # then keep run 1's .bgra/.json as the committed oracle.
    import vm  # harness/vm.py
    R = "pdngpu2"
    vm.run(r"ni -Force -ItemType Directory pdngpu2\scenes,pdngpu2\out1,pdngpu2\out2 >$null")
    vm.push(os.path.join(REPO, "harness/target/x86_64-pc-windows-gnu/release/d2d-ref.exe"), R + r"\d2d-ref.exe")
    for scene, sp in entries:
        vm.push(sp, f"{R}\\scenes\\{scene}.json")
    for run_i in (1, 2):
        for scene, _ in entries:
            vm.run(f'.\\{R}\\d2d-ref.exe .\\{R}\\scenes\\{scene}.json .\\{R}\\out{run_i}\\{scene}.png')
    allok = True
    for scene, _ in entries:
        bgra1 = os.path.join(oracle_out, f"{scene}.bgra")
        bgra2 = os.path.join(scene_out, f"{scene}.run2.bgra")
        vm.pull(f"{R}\\out1\\{scene}.bgra", bgra1)
        vm.pull(f"{R}\\out2\\{scene}.bgra", bgra2)
        a, b = open(bgra1, "rb").read(), open(bgra2, "rb").read()
        ok = a == b
        allok &= ok
        print(f"{'OK ' if ok else 'NON-DETERMINISTIC'} {scene} ({len(a)} B)")
        vm.pull(f"{R}\\out1\\{scene}.json", os.path.join(oracle_out, f"{scene}.json"))
    print("determinism:", "all identical" if allok else "MISMATCH -- do not commit")
    # Reclaim the VM's scratch (only 8 GB free; these are our own pdngpu2 files, pulled already).
    vm.run(r"ri -Recurse -Force pdngpu2\out1,pdngpu2\out2,pdngpu2\scenes,pdngpu2\d2d-ref.exe -EA SilentlyContinue")

main()
