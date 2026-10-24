#!/usr/bin/env python3
"""Generate R129 portable profiles offline from installed public .NET 10 assemblies.
Run: python3 scripts/number-cultures/generate.py > assets/lang/number-formats.json
Requires SDK 10.0.111 and runtime/reference pack 10.0.11. No NuGet restore.
"""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

root = Path(shutil.which('dotnet')).resolve().parent
source = Path(__file__).with_name('Program.cs').resolve()
refs = sorted((root / 'packs/Microsoft.NETCore.App.Ref/10.0.11/ref/net10.0').glob('*.dll'))
assert refs, 'Installed .NET 10.0.11 reference pack required'
with tempfile.TemporaryDirectory(prefix='paintrs-number-cultures-') as directory:
    work = Path(directory)
    assembly = work / 'number-cultures.dll'
    command = [str(root / 'dotnet'), str(root / 'sdk/10.0.111/Roslyn/bincore/csc.dll'),
               '-nologo', '-noconfig', '-nostdlib', '-target:exe', f'-out:{assembly}']
    result = subprocess.run(command + [f'-r:{ref}' for ref in refs] + [str(source)], capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    assembly.with_suffix('.runtimeconfig.json').write_text(json.dumps({
        'runtimeOptions': {'tfm': 'net10.0', 'framework': {'name': 'Microsoft.NETCore.App', 'version': '10.0.11'}}}))
    subprocess.run([str(root / 'dotnet'), str(assembly)], check=True)
