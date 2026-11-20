#!/usr/bin/env python3
"""Licence notices for everything the .NET plugin host ships in pdn/, into pdn/notices/: the .NET
runtime (a self-contained publish), the NuGet packages PdnBridge.deps.json lists, the third-party Rust
crates linked into opend2d's Direct2D/DirectWrite/WIC drop-ins, GDI+ (libgdiplus and
System.Drawing.Common 6.0) and Microsoft's chart control (System.Windows.Forms.DataVisualization, built
from its vendored MIT source). paint.rs's own code (the bridge, the replacement PaintDotNet.* assemblies
and the headless System.Windows.Forms) is under the distribution's LICENSE. Called by publish.sh; --strict (a release) fails on
anything shipped without its notice."""
import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

sys.dont_write_bytecode = True  # no __pycache__ in the source tree
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scripts' / 'ci'))
import collect_notices  # noqa: E402  (the release's crate-notice collector, reused for opend2d)

NOTICE_PREFIXES = ('license', 'licence', 'third-party-notices', 'thirdpartynotices', 'notice', 'copying')
# The vendored chart control's source, whose licence files ship beside its assembly.
DATAVIS = Path(__file__).resolve().parents[1] / 'paintnet' / 'assemblies' / 'WindowsFormsDataVisualization'
MIT = '''MIT License

{copyright}

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the
following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial
portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT
LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO
EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
USE OR OTHER DEALINGS IN THE SOFTWARE.
'''


def nuget_notices(deps_file, nuget, packs, out):
    """The runtime pack's and each package's notice files; a package that ships none (ComputeSharp)
    gets the MIT text its nuspec declares, with the nuspec's copyright line. A runtime pack that is not in the
    NuGet cache (the browser's, which the wasm-tools workload installs) is looked up in the SDK's packs/."""
    deps = json.loads(deps_file.read_text(encoding='utf-8'))
    names = []
    for key, lib in deps['libraries'].items():
        if lib['type'] not in ('package', 'runtimepack'):
            continue  # project/reference entries are paint.rs's own assemblies
        name, version = key.split('/')
        name = name.removeprefix('runtimepack.')
        root = nuget / name.lower() / version
        if not root.is_dir() and lib['type'] == 'runtimepack' and packs:
            root = packs / name / version
        if not root.is_dir():
            raise ValueError(f'{name} {version}: not in the NuGet cache at {root}')
        package_notice(root, name, version,
                       out / ('dotnet-runtime' if lib['type'] == 'runtimepack' else 'nuget') / f'{name}-{version}')
        names.append(f'{name} {version}')
    return names


def package_notice(root, name, version, dest):
    files = [p for p in root.iterdir() if p.is_file() and p.name.lower().startswith(NOTICE_PREFIXES)]
    dest.mkdir(parents=True, exist_ok=True)
    for f in files:
        shutil.copyfile(f, dest / f.name)
    if not any(f.name.lower().startswith(('license', 'licence')) for f in files):
        nuspec = next(root.glob('*.nuspec')).read_text(encoding='utf-8')
        expression = between(nuspec, '<license type="expression">', '</license>')
        if expression != 'MIT':
            raise ValueError(f'{name} {version}: no licence file and licence {expression!r} (only MIT is generated)')
        copyright = between(nuspec, '<copyright>', '</copyright>') or f'Copyright (c) {name} authors'
        (dest / 'LICENSE.txt').write_text(MIT.format(copyright=copyright), encoding='utf-8')


def between(text, start, end):
    i = text.find(start)
    return text[i + len(start):text.index(end, i)].strip() if i >= 0 else None


def opend2d_notices(opend2d, out):
    """Third-party crates in the normal/build closure of d2d-ffi, dwrite-ffi and wic-ffi (what links
    into libd2d1/libdwrite/libwindowscodecs). opend2d's own crates are its workspace members."""
    metadata = json.loads(subprocess.run(
        ['cargo', 'metadata', '--format-version', '1', '--manifest-path', str(opend2d / 'Cargo.toml')],
        check=True, capture_output=True).stdout)
    nodes = {n['id']: n for n in metadata['resolve']['nodes']}
    todo = [p['id'] for p in metadata['packages'] if p['name'] in ('d2d-ffi', 'dwrite-ffi', 'wic-ffi')]
    shipped = set()
    while todo:
        node = todo.pop()
        if node not in shipped:
            shipped.add(node)
            todo += [d['pkg'] for d in nodes[node]['deps'] if any(k['kind'] in (None, 'build') for k in d['dep_kinds'])]
    metadata['packages'] = [p for p in metadata['packages'] if p['id'] in shipped]
    collect_notices.collect_dependencies(metadata, out / 'opend2d')
    return sum(p['id'] not in metadata['workspace_members'] for p in metadata['packages'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pdn', type=Path)
    # The browser host: its deps file (in its publish folder), the SDK's packs/ (its runtime pack) and the
    # emscripten that relinked dotnet.native.wasm.
    parser.add_argument('--deps', type=Path)
    parser.add_argument('--packs', type=Path)
    parser.add_argument('--emscripten', type=Path)
    parser.add_argument('--nuget', type=Path, default=Path(os.environ.get('NUGET_PACKAGES', Path.home() / '.nuget/packages')))
    parser.add_argument('--opend2d', type=Path)
    parser.add_argument('--gdiplus-notice', type=Path)
    parser.add_argument('--strict', action='store_true')
    args = parser.parse_args()
    out = args.pdn / 'notices'
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    lines = ['# Notices for the .NET plugin host (pdn/)', '',
             "paint.rs's own code here (PdnBridge, the replacement PaintDotNet.* assemblies and the headless",
             "System.Windows.Forms) is under the distribution's LICENSE. The rest, with its licence notices in this",
             'folder:', '']
    lines += [f'- {n}' for n in nuget_notices(args.deps or args.pdn / 'PdnBridge.deps.json', args.nuget, args.packs, out)]
    # Mono.Cecil, which the browser host fetches beside itself (Mono.Cecil.bin) only for plug-ins it rewrites.
    if (args.pdn / 'Mono.Cecil.bin').exists():
        root = max((args.nuget / 'mono.cecil').iterdir())
        package_notice(root, 'Mono.Cecil', root.name, out / 'nuget' / f'Mono.Cecil-{root.name}')
        lines.append(f'- Mono.Cecil {root.name} (Mono.Cecil.bin)')
    # Roslyn, which the browser host fetches beside itself (Microsoft.CodeAnalysis*.bin) only for plug-ins that compile
    # C# while they run, and System.Configuration.ConfigurationManager, only for plug-ins that reference it.
    for package, name in (('microsoft.codeanalysis.common', 'Microsoft.CodeAnalysis'),
                          ('microsoft.codeanalysis.csharp', 'Microsoft.CodeAnalysis.CSharp'),
                          ('system.configuration.configurationmanager', 'System.Configuration.ConfigurationManager')):
        if (args.pdn / f'{name}.bin').exists():
            root = max((args.nuget / package).iterdir())
            package_notice(root, name, root.name, out / 'nuget' / f'{name}-{root.name}')
            lines.append(f'- {name} {root.name} ({name}.bin)')
    if args.emscripten:
        dest = out / 'emscripten'
        dest.mkdir()
        for name in ('LICENSE', 'AUTHORS'):
            shutil.copyfile(args.emscripten / name, dest / name)
        lines.append('- emscripten (its C library and runtime, linked into dotnet.native.wasm; emscripten/)')
    shipped_d2d = any((args.pdn / f).exists() for f in ('libd2d1.so', 'libd2d1.dylib', 'd2d1.dll'))
    if args.opend2d:
        lines.append(f'- opend2d (libd2d1, libdwrite, libwindowscodecs): {opend2d_notices(args.opend2d, out)} '
                     'third-party Rust crates, in opend2d/rust/ (inventory.json lists them)')
    elif shipped_d2d and args.strict:
        raise ValueError('opend2d libraries shipped but --opend2d not given')
    if any((args.pdn / f).exists() for f in ('System.Windows.Forms.DataVisualization.dll',
                                              '_framework/System.Windows.Forms.DataVisualization.wasm')):
        dest = out / 'System.Windows.Forms.DataVisualization'
        dest.mkdir()
        for name in ('LICENSE.TXT', 'THIRD-PARTY-NOTICES.TXT', 'PAINTRS-PATCH.md'):
            shutil.copyfile(DATAVIS / name, dest / name)
        lines.append('- System.Windows.Forms.DataVisualization (dotnet/winforms-datavisualization, MIT; '
                     'System.Windows.Forms.DataVisualization/)')
    if any(args.pdn.glob('libgdiplus.*')):
        if args.gdiplus_notice and args.gdiplus_notice.is_file():
            shutil.copyfile(args.gdiplus_notice, out / 'GDIPLUS-NOTICE.txt')
            lines.append('- libgdiplus (with giflib linked in) and System.Drawing.Common 6.0 (GDIPLUS-NOTICE.txt)')
        elif args.strict:
            raise ValueError('libgdiplus shipped without its notice (scripts/ci/build-gdiplus.sh writes it)')
        else:
            print('warning: libgdiplus shipped without its notice', file=sys.stderr)
    (out / 'README.md').write_text('\n'.join(lines) + '\n', encoding='utf-8')
    print(f'notices -> {out}')


if __name__ == '__main__':
    try:
        main()
    except ValueError as error:
        sys.exit(f'error: {error}')
