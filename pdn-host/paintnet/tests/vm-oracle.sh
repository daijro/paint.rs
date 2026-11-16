#!/usr/bin/env sh
# Byte oracle: render each plugin through genuine Paint.NET on real Windows (real GDI+/D2D) and
# byte-compare against the Linux replacement. This is the authoritative check the Wine path cannot
# give for GDI+ drawing (Wine's GDI+ is its own code).
#
# Why it is shaped this way:
#   - The genuine assemblies in opend2d's ref/pdn-wine are net11; the reference VM's unserviced OS
#     (LTSC 26100.0) cannot initialise ANY net11 coreclr (0xC0000142), so that build is Wine-only.
#   - BUT genuine Paint.NET 5.1.12 (net9) is installed on the VM at C:\Program Files\paint.net and
#     runs. So we drive pdnhost net9 *framework-dependent* on PDN's own runtime + assemblies,
#     read-only, copying them into our own C:\d2d-rs\pdn1 area (never touching the install).
#   - Each effect runs in its OWN vm.py call (kill any prior pdnhost, 2s settle). Running several
#     back-to-back in one session trips a transient "C:\d2d-rs\pdn1\work access denied" on load.
#   - pdnhost.runtimeconfig.json is the self-contained form (no "framework" section) so PDN's
#     app-local coreclr is used; pdnhost.deps.json is omitted so the host probes the app dir.
#
# Prereqs: opend2d checkout (for harness/vm.py and the reference VM), the net9 host published to
# .build/host-win9 (publish.sh/this script builds it), the plugins + a 48x48 BGRA input, and the
# Linux outputs already rendered (tests/run.py or classify). Serialise with flock /tmp/d2d-vm.lock.
set -eu
here="$(cd "$(dirname "$0")/.." && pwd)"                       # pdn-host/paintnet
vmpy="${PDN_VMPY:-/home/alex/Documents/opend2d/harness/vm.py}"
pdninstall='C:\Program Files\paint.net'
B="$here/.build"; plugins="$here/../../ref/pdn-plugins/boltbait/bin"
inp="${PDN_ORACLE_IN:?set PDN_ORACLE_IN to a 48x48 BGRA file}"
lin="${PDN_ORACLE_LINUX:?set PDN_ORACLE_LINUX to the dir of Linux .bgra outputs}"
out="$B/oracle-vm"; mkdir -p "$out"
vm() { python3 "$vmpy" "$@"; }

# 1. Publish the net9 framework-dependent host and rewrite its runtimeconfig to self-contained form.
sed -i 's#<TargetFramework>net10.0</TargetFramework>#<TargetFramework>net9.0</TargetFramework>#' host/pdnhost.csproj
./dn.sh publish host/pdnhost.csproj -c Release -r win-x64 --self-contained false -o "$B/host-win9" -m:1 --disable-build-servers -nodeReuse:false
sed -i 's#<TargetFramework>net9.0</TargetFramework>#<TargetFramework>net10.0</TargetFramework>#' host/pdnhost.csproj
printf '{\n  "runtimeOptions": { "tfm": "net9.0", "rollForward": "LatestMinor",\n    "configProperties": { "System.Globalization.Invariant": true, "System.Globalization.PredefinedCulturesOnly": true } }\n}\n' > "$B/host-win9/pdnhost.runtimeconfig.json"
rm -f "$B/host-win9/pdnhost.deps.json"

# 2. Stage once: PDN's own DLLs (runtime + genuine assemblies) + our host + plugins + input.
flock /tmp/d2d-vm.lock sh -c "
  python3 '$vmpy' run \"New-Item -ItemType Directory -Force C:\\d2d-rs\\pdn1\\pdnrun | Out-Null; Copy-Item '$pdninstall\\*.dll' C:\\d2d-rs\\pdn1\\pdnrun\\ -Force\"
  for f in pdnhost.dll pdnhost.exe pdnhost.runtimeconfig.json; do python3 '$vmpy' push '$B/host-win9/'\$f pdn1/pdnrun/\$f; done
  python3 '$vmpy' push '$inp' pdn1/work/in.bgra
"

# 3. Each effect: isolated run (kill prior pdnhost, settle), pull, compare.  tag|dll|effect
for s in \
  "FlipH|Flip.dll|Flip.FlipHEffect" \
  "FlipV|Flip.dll|Flip.FlipVEffect" \
  "HSVGradients|HSVGradients.dll|HSVGradientsEffect.GradientEffectEffectPlugin" \
  "Polygon|Polygon.dll|PolygonEffect.PolygonEffectPlugin" \
  "ComplementaryColors|ComplementaryColors.dll|ComplementaryColorsEffect.ComplementaryColorsEffectPlugin" \
  "GridCheckerboard|GridCheckerboard.dll|GridCheckerboardEffect.GridCheckerboardEffectPlugin" \
  "HueSatPlus|HueSatPlus.dll|HueSatPlusEffect.HueSatPlusEffectPlugin"; do
  tag=${s%%|*}; rest=${s#*|}; dll=${rest%%|*}; eff=${rest#*|}
  flock /tmp/d2d-vm.lock sh -c "
    python3 '$vmpy' push '$plugins/$dll' pdn1/work/$dll >/dev/null
    python3 '$vmpy' run \"Get-Process pdnhost -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Seconds 2; cmd /c 'C:\\d2d-rs\\pdn1\\pdnrun\\pdnhost.exe --plugin C:\\d2d-rs\\pdn1\\work\\$dll --asmdir C:\\d2d-rs\\pdn1\\pdnrun --effect $eff --in C:\\d2d-rs\\pdn1\\work\\in.bgra --out C:\\d2d-rs\\pdn1\\work\\$tag.vm.bgra --width 48 --height 48'\"
    python3 '$vmpy' pull pdn1/work/$tag.vm.bgra '$out/$tag.bgra'
  "
  if cmp -s "$lin/$tag.bgra" "$out/$tag.bgra"; then echo "$tag: IDENTICAL"; else echo "$tag: DIFFER"; fi
done
