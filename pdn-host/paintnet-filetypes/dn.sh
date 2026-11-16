#!/usr/bin/env sh
# Thin `dotnet` wrapper: keep every cache the SDK/NuGet touch inside the gitignored
# .build directory. The dev sandbox makes ~/.dotnet, ~/.nuget and /run/user read-only
# and exposes the worktree's .gitmodules as a device node, so a plain `dotnet build`
# fails confusingly without these. Build/restore also need
# `--disable-build-servers -nodeReuse:false` (the out-of-process MSBuild node cannot
# complete its pipe handshake here). Mirrors hosts/paintnet/dn.sh.
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$here/.build/run" "$here/.build/tmp"
export DOTNET_CLI_HOME="$here/.build/clihome" \
       NUGET_PACKAGES="$here/.build/nuget" \
       NUGET_HTTP_CACHE_PATH="$here/.build/httpcache" \
       XDG_DATA_HOME="$here/.build/xdg" \
       XDG_RUNTIME_DIR="$here/.build/run" \
       TMPDIR="$here/.build/tmp" \
       DOTNET_SKIP_FIRST_TIME_EXPERIENCE=1 \
       DOTNET_NOLOGO=1 \
       DOTNET_CLI_TELEMETRY_OPTOUT=1 \
       MSBUILDDISABLENODEREUSE=1 \
       DOTNET_CLI_DO_NOT_USE_MSBUILD_SERVER=1
exec dotnet "$@"
