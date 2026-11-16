#!/usr/bin/env sh
# Publish the .NET plugin host into an app-local `pdn/` folder that paint.rs finds
# beside its executable (or via $PDN_BRIDGE_DIR). Two modes:
#
#   ./publish.sh <out-dir>                 framework-dependent (needs .NET 11 installed)
#   ./publish.sh <out-dir> <runtime-id>    self-contained (bundles the runtime; no .NET needed)
#                                          runtime-id e.g. linux-x64, win-x64, osx-arm64
#
# Output: <out-dir>/ with PdnBridge.dll, PdnBridge.runtimeconfig.json, the replacement
# PaintDotNet.*.dll and deps. Point paint.rs at it with PDN_BRIDGE_DIR=<out-dir>, or copy
# it to `pdn/` next to the binary in the app bundle.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
out="${1:?usage: publish.sh <out-dir> [runtime-id]}"
rid="${2:-}"
# $OPEND2D_DIR names the opend2d checkout whose d2d1/dwrite/windowscodecs this builds (below). There is no
# default, so a publish never builds in a checkout nobody pointed it at: unset, it stops before building anything.
: "${OPEND2D_DIR:?set OPEND2D_DIR to the opend2d checkout to build the native libraries from}"

common="-c Release -m:1 --disable-build-servers -nodeReuse:false -o $out"
if [ -n "$rid" ]; then
    "$here/dn.sh" publish "$here/PdnBridge.csproj" $common -r "$rid" --self-contained true -p:PublishTrimmed=false
else
    "$here/dn.sh" publish "$here/PdnBridge.csproj" $common
fi
out="$(cd "$out" && pwd)"   # absolute, so the native build below can cp into it

# --- native graphics libraries (deliverable: GPU plugins over opend2d) ---
# opend2d is consumed as a library, kept unmodified: build its d2d1/dwrite/windowscodecs cdylib drop-ins
# and ship them beside PdnBridge.dll, where NativeResolver binds the DllImport names "d2d1"/"dwrite"
# that the replacement PaintDotNet.Direct2D1/DirectWrite wrappers import.
opend2d="$OPEND2D_DIR"
opend2d_notices=
if [ -f "$opend2d/Cargo.toml" ]; then
    case "$(uname -s)" in Darwin) ext=dylib ;; *) ext=so ;; esac
    opend2d="$(cd "$opend2d" && pwd)"

    # opend2d builds into its own target dir (kept out of paint.rs's, even under CARGO_TARGET_DIR, so the
    # copy below never ships a stale build); release cdylibs only.
    cargo build --release --manifest-path "$opend2d/Cargo.toml" --target-dir "$opend2d/target" -p d2d-ffi -p dwrite-ffi -p wic-ffi
    for n in d2d1 dwrite windowscodecs; do cp "$opend2d/target/release/lib$n.$ext" "$out/"; done
    echo "native libs -> $out (d2d1, dwrite, windowscodecs)"
    opend2d_notices="$opend2d"
elif [ -n "$rid" ]; then
    # Self-contained == a release bundle (release.yml passes a RID). A release host
    # with no graphics libs silently can't render the D2D/GPU effects, so fail loudly
    # instead: point OPEND2D_DIR at an opend2d checkout (release jobs: scripts/ci/fetch-opend2d.sh).
    echo "error: opend2d not found at \$OPEND2D_DIR ($opend2d); a self-contained release" >&2
    echo "       needs libd2d1/libdwrite/libwindowscodecs. Set OPEND2D_DIR and retry." >&2
    exit 1
else
    echo "warning: opend2d not found at \$OPEND2D_DIR ($opend2d); GPU plugins will not render" >&2
fi

# --- GDI+ off Windows (plugins that load an icon or draw) ---
# System.Drawing.Common 10.0 only runs on Windows' gdiplus.dll; elsewhere swap in its 6.0 unix build
# and the libgdiplus it P/Invokes, as pdnhost's shim does (paintnet/tests/stage-gdiplus.sh, whose
# PDN_BUILD_TMP / PDN_SDC_UNIX / PDN_LIBGDIPLUS name the inputs; scripts/ci/build-gdiplus.sh makes them
# for a release). A release (a RID) without them fails, as without opend2d: nearly every plugin would.
case "$rid" in win-*) ;; *)
    case "$(uname -s)" in Linux|Darwin)
        if ! sh "$here/../paintnet/tests/stage-gdiplus.sh" "$out"; then
            [ -z "$rid" ] || { echo "error: GDI+ inputs missing for a release; run scripts/ci/build-gdiplus.sh" >&2; exit 1; }
            echo "warning: GDI+ inputs missing; plugins that use System.Drawing will not load" >&2
        fi
        # Stand-ins plugins bind instead of framework assemblies that do not work here (Microsoft.Win32.Registry):
        # plugin-shims/, which the plugin load context checks first.
        "$here/dn.sh" build "$here/../paintnet/assemblies/Win32Registry/Win32Registry.csproj" -c Release -m:1 \
            --disable-build-servers -nodeReuse:false -o "$out/plugin-shims" >/dev/null || exit 1
        # SystemEvents that does not throw off Windows, in place of the package's (Win32SystemEvents)
        "$here/dn.sh" build "$here/../paintnet/assemblies/Win32SystemEvents/Win32SystemEvents.csproj" -c Release -m:1 \
            --disable-build-servers -nodeReuse:false -o "$out" >/dev/null || exit 1
        # WPF's WindowsBase and PresentationCore types plugins use (Point, EllipseGeometry), which .NET has not.
        "$here/dn.sh" build "$here/../paintnet/assemblies/PresentationCore/PresentationCore.csproj" -c Release -m:1 \
            --disable-build-servers -nodeReuse:false -o "$out/plugin-shims" >/dev/null || exit 1
        # user32's GetSystemMetrics and window calls, which some plugins' dialogs P/Invoke (native/user32.c)
        case "$(uname -s)" in Darwin) cc -dynamiclib -O2 -o "$out/libuser32.dylib" "$here/native/user32.c" ;;
                               *) cc -shared -fPIC -O2 -o "$out/libuser32.so" "$here/native/user32.c" ;; esac ;;
    esac ;;
esac

echo "published -> $out ($(du -sh "$out" | cut -f1))"

# --- licence notices for everything in $out (pdn/notices/; a release fails on a missing one) ---
"${PYTHON:-python3}" "$here/collect-notices.py" "$out" --nuget "$here/.build/nuget" \
    ${opend2d_notices:+--opend2d "$opend2d_notices"} ${PDN_GDIPLUS_NOTICE:+--gdiplus-notice "$PDN_GDIPLUS_NOTICE"} \
    ${rid:+--strict}
