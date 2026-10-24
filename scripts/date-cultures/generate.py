#!/usr/bin/env python3
"""Generate R154 Calendar date profiles offline from installed public .NET 10 assemblies.
Run: python3 scripts/date-cultures/generate.py
Writes assets/lang/date-formats.json and the Q59 runtime parse fixtures beside calendar.rs.
Uses the installed SDK 10.0.1xx and the matching runtime/reference pack 10.0.x (R154 was prepared
on 10.0.11; the exact versions are recorded in the output). No NuGet restore, no network.
"""
import hashlib
import json
from pathlib import Path
import os
import shutil
import subprocess
import tempfile

here = Path(__file__).resolve().parent
repo = here.parents[1]
root = Path(shutil.which('dotnet')).resolve().parent
source = here / 'Program.cs'
[pack] = sorted((root / 'packs/Microsoft.NETCore.App.Ref').glob('10.0.*'))
[sdk] = sorted((root / 'sdk').glob('10.0.1*'))
assert (root / 'shared/Microsoft.NETCore.App' / pack.name).is_dir(), 'matching .NET 10 runtime required'
refs = sorted((pack / 'ref/net10.0').glob('*.dll'))
assert refs, 'Installed .NET 10 reference pack required'
env = dict(os.environ, DOTNET_SYSTEM_GLOBALIZATION_INVARIANT='0', DOTNET_CLI_TELEMETRY_OPTOUT='1',
           DOTNET_SKIP_FIRST_TIME_EXPERIENCE='1')
with tempfile.TemporaryDirectory(prefix='paintrs-date-cultures-') as directory:
    work = Path(directory)
    env['DOTNET_CLI_HOME'] = str(work)
    assembly = work / 'date-cultures.dll'
    command = [str(root / 'dotnet'), str(sdk / 'Roslyn/bincore/csc.dll'),
               '-nologo', '-noconfig', '-nostdlib', '-deterministic+', '-target:exe', f'-out:{assembly}']
    result = subprocess.run(command + [f'-r:{ref}' for ref in refs] + [str(source)],
                            capture_output=True, text=True, env=env)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    assembly.with_suffix('.runtimeconfig.json').write_text(json.dumps({'runtimeOptions': {
        'tfm': 'net10.0', 'rollForward': 'Disable',
        'framework': {'name': 'Microsoft.NETCore.App', 'version': pack.name}}}))
    output = json.loads(subprocess.run([str(root / 'dotnet'), str(assembly), sdk.name], check=True,
                                       capture_output=True, text=True, env=env).stdout)

digest = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
output['source']['generator'] = {'Program.cs': digest(source), 'generate.py': digest(here / 'generate.py')}
fixtures = {'source': output['source'], 'fixtures': output.pop('fixtures')}
dump = lambda v: json.dumps(v, ensure_ascii=False, separators=(',', ':')) + '\n'
(repo / 'assets/lang/date-formats.json').write_text(dump(output), encoding='utf-8')
(repo / 'crates/pdn-core/src/effect/bb/calendar_parse_fixtures.json').write_text(
    json.dumps(fixtures, ensure_ascii=False, indent=1) + '\n', encoding='utf-8')
