#!/usr/bin/env python3
"""Copy distribution notices without copying source, binaries, or private icon packs."""
import argparse
import json
import hashlib
from pathlib import Path
import shutil
import tomllib

NATIVE_CRATES = {'heic-rs', 'dav1d', 'dav1d-sys', 'jpegxl-src', 'libwebp-sys',
                 'openreadout-jpegxr', 'jpeg-encoder'}


def is_notice(path):
    name = path.name.lower()
    return (path.suffix.lower() not in {'.rs', '.c', '.h', '.cc', '.cpp', '.cxx', '.hpp',
                                      '.go', '.java', '.js', '.mjs', '.cjs', '.py', '.rb',
                                      '.ts', '.tsx', '.jsx', '.swift', '.m', '.mm'}
            and name.startswith(('license', 'licence', 'copying', 'notice', 'ofl', 'copyright', 'authors')))


def copy(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)


def collect_project(root, destination):
    for name in ('LICENSE', 'LICENSE.md', 'NOTICE.md'):
        copy(root / name, destination / name)
    for source in (root / 'assets').rglob('*'):
        if source.is_file() and is_notice(source):
            copy(source, destination / source.relative_to(root))


def collect_native(packages, destination, dav1d_license):
    packages = {p['name']: p for p in packages if p['name'] in NATIVE_CRATES}
    if set(packages) != NATIVE_CRATES or not dav1d_license.is_file():
        raise ValueError('Missing native codec package or system dav1d license')
    for name, package in packages.items():
        root = Path(package['manifest_path']).parent
        notices = [path for path in root.rglob('*') if path.is_file() and is_notice(path)]
        if not notices:
            raise ValueError(f'Missing codec notices: {name}')
        for source in notices:
            copy(source, destination / 'codecs' / name / source.relative_to(root))
    copy(dav1d_license, destination / 'codecs/system-dav1d/COPYING')


def supplemental_notices(package, supplement_root):
    manifest = supplement_root / 'manifest.json'
    if not manifest.is_file():
        return []
    groups = json.loads(manifest.read_bytes())['groups']
    for group in groups:
        expected = next((p for p in group['packages'] if p['name'] == package['name']
                         and p['version'] == package['version']), None)
        if expected is None:
            continue
        root = Path(package['manifest_path']).parent
        vcs = root / '.cargo_vcs_info.json'
        if expected['license'] != package.get('license') or not vcs.is_file() or json.loads(vcs.read_bytes())['git']['sha1'] != group['package_commit']:
            raise ValueError('Supplement provenance does not match published package')
        if 'authors' in expected and package.get('authors') != expected['authors']:
            raise ValueError('Supplement does not match published authors')
        if group.get('ruling') in ('R134', 'R135', 'R141'):
            lock = tomllib.loads((Path(__file__).resolve().parents[2] / 'Cargo.lock').read_text(encoding='utf-8'))
            matches = [p for p in lock['package'] if p['name'] == package['name']
                       and p['version'] == package['version'] and p.get('source') == group['registry_source']]
            if package.get('source') != group['registry_source'] or len(matches) != 1 or matches[0]['checksum'] != group['cargo_lock_checksum']:
                raise ValueError(group['ruling'] + ' registry or Cargo.lock checksum changed')
            for name, digest in group['package_file_sha256'].items():
                if not (root / name).is_file() or hashlib.sha256((root / name).read_bytes()).hexdigest() != digest:
                    raise ValueError(group['ruling'] + ' preserved metadata file changed: ' + name)
        result = []
        for notice in group['notices']:
            source = supplement_root / notice['file']
            if not source.is_file():
                raise ValueError('Missing reviewed notice: ' + notice['url'])
            raw = source.read_bytes()
            blob = hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest()
            if len(raw) != notice['bytes'] or blob != notice['git_blob_sha1'] or hashlib.sha256(raw).hexdigest() != notice['sha256']:
                raise ValueError('Changed reviewed notice: ' + notice['file'])
            if 'package_file' in notice and (not (root / notice['package_file']).is_file() or hashlib.sha256((root / notice['package_file']).read_bytes()).hexdigest() != notice['sha256']):
                raise ValueError('Preserved package metadata changed: ' + notice['package_file'])
            result.append((source, notice, group))
        return result
    return []


def collect_dependencies(metadata, destination, supplement_root=None):
    """Retain the full metadata superset, including build/test and other-target crates."""
    inventory = dict(scope='Unfiltered Cargo metadata superset; not a linked dependency closure',
                     packages=[], missing=[])
    workspace = set(metadata['workspace_members'])
    supplement_root = supplement_root or Path(__file__).with_name('license-supplements')
    for package in metadata['packages']:
        if package['id'] in workspace:
            continue  # Project notices cover the first-party workspace packages.
        root = Path(package['manifest_path']).resolve().parent
        key = package['name'] + '-' + package['version']
        files = [path for path in root.rglob('*') if path.is_file() and is_notice(path)]
        declared = package.get('license_file')
        license_path = Path(declared) if declared else None
        if license_path is not None and not license_path.is_absolute():
            license_path = root / license_path
        entry = {k: package.get(k) for k in ('name', 'version', 'source', 'repository', 'license', 'authors')}
        entry.update(license_file=declared, notices=[])
        supplements = []
        try:
            supplements = supplemental_notices(package, supplement_root)
        except ValueError as error:
            entry['error'] = str(error)
        if license_path is not None:
            if not license_path.resolve().is_relative_to(root):
                entry['error'] = 'Cargo license_file escapes the package root'
            elif not license_path.is_file() or not license_path.stat().st_size:
                entry['error'] = 'Missing or empty Cargo license_file'
            elif license_path not in files:
                files.append(license_path)
        elif not package.get('license'):
            entry['error'] = 'No Cargo license expression or license_file'
        elif not any(notice.get('role') == 'license-terms' for _, notice, _ in supplements) and not any(path.name.lower().startswith(('license', 'licence', 'copying', 'ofl'))
                     and path.parent == root and path.stat().st_size for path in files):
            entry.setdefault('error', 'Cargo license expression has no supplied license text')
        for source in sorted(files):
            if not source.resolve().is_relative_to(root):
                entry['error'] = 'Notice symlink escapes the package root'
                continue
            relative = source.relative_to(root) if source.is_relative_to(root) else Path('cargo-license-file') / source.name
            target = Path('rust') / key / relative
            copy(source, destination / target)
            entry['notices'].append(dict(path=str(target), source_path=str(relative),
                                         sha256=hashlib.sha256(source.read_bytes()).hexdigest()))
        for source, notice, group in supplements:
            target = Path('rust') / key / 'upstream' / notice['path']
            copy(source, destination / target)
            record = dict(path=str(target), source_url=notice['url'],
                          git_blob_sha1=notice['git_blob_sha1'], sha256=notice['sha256'],
                          provenance_basis=group['basis'],
                          provenance_category=notice.get('category', group.get('category', 'exact-original')),
                          role=notice['role'])
            if 'provenance' in group:
                record['mapping_provenance'] = group['provenance']
            for field in ('derivation', 'source_package'):
                if field in notice:
                    record[field] = notice[field]
            entry['notices'].append(record)
        inventory['packages'].append(entry)
        if 'error' in entry:
            inventory['missing'].append({k: entry[k] for k in ('name', 'version', 'license', 'license_file', 'error')})
    target = destination / 'rust/inventory.json'
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(inventory, indent=2) + '\n', encoding='utf-8')
    if inventory['missing']:
        raise ValueError('Missing required dependency notices: ' + ', '.join(
            p['name'] + '@' + p['version'] for p in inventory['missing']))


def collect(root, metadata, destination, kind, dav1d_license=None):
    collect_project(root, destination)
    if kind == 'native':
        if dav1d_license is None:
            raise ValueError('Native distributions require the actual system dav1d license')
        collect_native(metadata['packages'], destination, dav1d_license)
    collect_dependencies(metadata, destination)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', type=Path)
    parser.add_argument('--metadata', type=Path, required=True)
    parser.add_argument('--kind', choices=('native', 'web'), required=True)
    parser.add_argument('--dav1d-license', type=Path)
    args = parser.parse_args()
    collect(Path(__file__).resolve().parents[2], json.loads(args.metadata.read_bytes()),
            args.destination, args.kind, args.dav1d_license)
