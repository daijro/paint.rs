#!/usr/bin/env sh
# Stage the three GDI+ bits the classic effect host needs into the shim asmdir, so a plugin's
# System.Drawing use resolves entirely inside the plugin load context (never leaking to the
# default ALC). Run after `run.py build`. Idempotent. All three are external, not committed.
# `stage-gdiplus.sh <dir>` stages just 1 and 2 into a published pdn-bridge (bridge/publish.sh),
# which loads the facade from the runtime itself:
#
#   1. System.Drawing.Common 6.0 (unix build) — the last with the libgdiplus P/Invoke. From the
#      6.0 nupkg's runtimes/unix/lib/net6.0/. (System.Drawing.EnableUnixSupport=true is set in
#      pdnhost Main; the single ALC resolves System.Drawing.Common by name to this file.)
#   2. libgdiplus.so (.dylib on macOS) — built from mono/libgdiplus (see README; release builds:
#      scripts/ci/build-gdiplus.sh). System.Drawing.Common 6.0 P/Invokes it.
#   3. System.Drawing.dll — the framework's type-forwarder facade (System.Drawing -> Common/Primitives).
#      Classic Paint.NET 4.x plugins reference the old .NET Framework System.Drawing assembly
#      (PublicKeyToken b03f5f7f…) for Image/Bitmap/Graphics/… Staging the facade in the asmdir makes
#      the plugin-load-context resolve it (and so its forwards to our 6.0 System.Drawing.Common),
#      unifying the plugin's Image with the replacement Effect's Image. Without it the custom ALC
#      returns null for System.Drawing (a framework TPA assembly), the reference escapes to the
#      default ALC, and that pulls in a *second* System.Drawing.Common -> MissingMethod on the
#      Effect(string, Image, string, EffectOptions) ctor.
set -eu
here="$(cd "$(dirname "$0")/.." && pwd)"               # pdn-host/paintnet
shim="${1:-$here/.build/shim}"
tmp="${PDN_BUILD_TMP:-$here/../../.build-tmp}"          # where the nupkg/gdiplus were fetched/built

sdc="${PDN_SDC_UNIX:-$tmp/sdc/sdc/runtimes/unix/lib/net6.0/System.Drawing.Common.dll}"
case "$(uname -s)" in Darwin) ext=dylib; built=libgdiplus.0.dylib ;; *) ext=so; built=libgdiplus.so.0.0.0 ;; esac
gdiplus="${PDN_LIBGDIPLUS:-$tmp/gdiplus/install/lib/$built}"
# The facade ships in every shared .NET runtime; pick the newest installed Microsoft.NETCore.App.
facade="${PDN_SYSTEM_DRAWING:-$(ls -d /usr/share/dotnet/shared/Microsoft.NETCore.App/*/System.Drawing.dll 2>/dev/null | sort -V | tail -1)}"

[ -d "$shim" ] || { echo "target not built yet: $shim (run run.py build first)" >&2; exit 1; }
for f in "$sdc" "$gdiplus"; do
    [ -f "$f" ] || { echo "missing GDI+ staging input: $f" >&2; exit 1; }
done
[ $# -gt 0 ] || [ -f "$facade" ] || { echo "missing GDI+ staging input: $facade" >&2; exit 1; }
cp "$sdc"     "$shim/System.Drawing.Common.dll"
cp "$gdiplus" "$shim/libgdiplus.$ext"
[ $# -gt 0 ] || cp "$facade" "$shim/System.Drawing.dll"
echo "staged GDI+ bits into $shim"
