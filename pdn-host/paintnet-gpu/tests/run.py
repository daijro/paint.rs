#!/usr/bin/env python3
"""Build and run the Paint.NET GPU-plugin host (phase 5A-5/5A-6).

    build    native bridge (cargo) + the unified PaintDotNet set + plugin + host (dotnet)
    run      render the test effect through the host and byte-compare to the WARP capture
    binding  prove Paint.NET's genuine Direct2D interop mechanism binds to our portable libd2d1
    all      build then run then binding

The shader the test effect ships (authored shader-06) already has a committed WARP capture in
conformance/oracle, so `run` needs no VM round-trip: a byte-exact match there is a byte-exact match
against native Direct2D on WARP (crates/d2d-shader is validated against the same capture).

`binding` builds the portable libd2d1 (d2d-ffi, off-Windows COM vtables) and drives it from .NET the
way Paint.NET's genuine TerraFX/ComObject wrappers do (DllImport("d2d1") + member-function vtable
calls, no ComWrappers); see hosts/paintnet-gpu/binding.
"""
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
HOST = os.path.dirname(HERE)              # hosts/paintnet-gpu
ROOT = os.path.dirname(os.path.dirname(HOST))  # repo root
DN = os.path.join(HOST, "dn.sh")
NATIVE_TARGET = os.path.join(HOST, ".build", "native")
HOSTBIN = os.path.join(HOST, "host", "bin", "Release", "net10.0")
PLUGIN = os.path.join(HOST, "plugins", "SampleGpuEffect", "bin", "Release", "net10.0", "SampleGpuEffect.dll")
SCENE = os.path.join(ROOT, "harness", "scenes", "shader-06-sample-nearest.json")
ORACLE = os.path.join(ROOT, "conformance", "oracle", "win11-26100", "shader-06-sample-nearest.bin")
BINDBIN = os.path.join(HOST, "binding", "bin", "Release", "net10.0", "pdngpu-binding.dll")
LIBD2D1 = os.path.join(ROOT, "target", "debug", "libd2d1.so")
DN_FLAGS = ["--disable-build-servers", "-nodeReuse:false"]


def sh(cmd, **kw):
    print("+", " ".join(cmd))
    subprocess.run(cmd, check=True, **kw)


def build():
    env = dict(os.environ, CARGO_TARGET_DIR=NATIVE_TARGET)
    sh(["cargo", "build", "--release", "--offline", "--manifest-path",
        os.path.join(HOST, "native", "d2dgpu", "Cargo.toml")], env=env)
    # The portable libd2d1 (off-Windows COM vtables) for the binding proof.
    sh(["cargo", "build", "-p", "d2d-ffi"], cwd=ROOT)
    # One restore needs the network (net10 ref/host packs + the shared PaintDotNet set); later
    # builds are offline. host/plugin pull the unified assemblies set via ProjectReference.
    projs = ("host/pdngpu-host.csproj", "plugins/SampleGpuEffect/SampleGpuEffect.csproj", "binding/binding.csproj")
    for proj in projs:
        sh([DN, "restore", os.path.join(HOST, proj)] + DN_FLAGS)
    for proj in projs:
        sh([DN, "build", os.path.join(HOST, proj), "-c", "Release", "--no-restore"] + DN_FLAGS)
    os.makedirs(HOSTBIN, exist_ok=True)
    shutil.copy(os.path.join(NATIVE_TARGET, "release", "libd2dgpu.so"), HOSTBIN)


def run():
    sh([DN, os.path.join(HOSTBIN, "pdngpu-host.dll"), PLUGIN, SCENE, ORACLE])


def binding():
    sh([DN, BINDBIN], env=dict(os.environ, LIBD2D1=LIBD2D1))


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "all"
    if cmd in ("build", "all"):
        build()
    if cmd in ("run", "all"):
        run()
    if cmd in ("binding", "all"):
        binding()
