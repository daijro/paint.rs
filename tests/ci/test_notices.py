"""Distribution notices retain source bytes and native codec notices."""
import importlib.util
import codecs
import json
import hashlib
import shutil
import os
import subprocess
import sys
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('notices', ROOT / 'scripts/ci/collect_notices.py')
notices = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(notices)

class NoticesTests(unittest.TestCase):
    def test_project_and_asset_licenses_keep_bytes_and_paths(self):
        with tempfile.TemporaryDirectory() as tmp:
            destination = Path(tmp)
            notices.collect_project(ROOT, destination)
            for rel in ['NOTICE.md', 'LICENSE', 'LICENSE.md', 'assets/font/selawik/LICENSE.txt',
                        'assets/codecs/magick/NOTICE', 'assets/codecs/jpegxr/license.md']:
                self.assertEqual((destination / rel).read_bytes(), (ROOT / rel).read_bytes())
            self.assertFalse((destination / 'assets/font/selawik/selawk.ttf').exists())

    def test_implementation_filenames_are_not_notices(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            source = tmp / 'source'
            (source / 'src').mkdir(parents=True)
            (source / 'src/copying.rs').write_text('pub trait Copying {}')
            package = dict(id='example', name='example', version='1', source='test-registry',
                           manifest_path=str(source / 'Cargo.toml'), license='MIT', license_file=None)
            with self.assertRaises(ValueError):
                notices.collect_dependencies(dict(packages=[package], workspace_members=[]), tmp / 'output')
            self.assertFalse((tmp / 'output/rust/example-1/src/copying.rs').exists())

    def test_nested_third_party_terms_do_not_replace_package_terms(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            source = tmp / 'source'
            (source / 'vendor').mkdir(parents=True)
            (source / 'vendor/COPYING').write_text('Bundled third-party BSD terms')
            package = dict(id='example', name='example', version='1', source='test-registry',
                           manifest_path=str(source / 'Cargo.toml'), license='MIT', license_file=None)
            with self.assertRaises(ValueError):
                notices.collect_dependencies(dict(packages=[package], workspace_members=[]), tmp / 'output')
            self.assertEqual((tmp / 'output/rust/example-1/vendor/COPYING').read_text(),
                             'Bundled third-party BSD terms')

    def test_missing_native_licenses_fail_packaging(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(ValueError):
                notices.collect_native([], Path(tmp), Path(tmp) / 'missing-dav1d')

    def test_tiny_skia_license_is_preserved_in_both_package_modes(self):
        original = (ROOT / 'tests/ci/fixtures/tiny-skia-0.12.0-LICENSE').read_bytes()
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            packages = []
            for name in ['tiny-skia', *sorted(notices.NATIVE_CRATES)]:
                source = tmp / name
                source.mkdir()
                (source / 'LICENSE').write_bytes(original)
                packages.append(dict(id=name, name=name, version='0.12.0', source='test-registry',
                                     manifest_path=str(source / 'Cargo.toml'), license='BSD-3-Clause',
                                     license_file=None, repository='https://example.invalid/test'))
            metadata = dict(packages=packages, workspace_members=[])
            dav1d = tmp / 'dav1d-COPYING'
            dav1d.write_bytes(original)
            for kind in ['web', 'native']:
                destination = tmp / kind
                notices.collect(ROOT, metadata, destination, kind, dav1d if kind == 'native' else None)
                self.assertEqual((destination / 'rust/tiny-skia-0.12.0/LICENSE').read_bytes(), original)
                inventory = json.loads((destination / 'rust/inventory.json').read_text())
                self.assertIn('superset', inventory['scope'])
                self.assertFalse(inventory['missing'])

    def test_cli_preserves_non_ascii_metadata_under_non_utf8_locale(self):
        environment = dict(os.environ, PYTHONUTF8='0', PYTHONCOERCECLOCALE='0',
                           LC_ALL='C', PYTHONDONTWRITEBYTECODE='1')
        probe = subprocess.check_output([sys.executable, '-c',
            'import locale, sys; assert sys.flags.utf8_mode == 0; '
            'print(locale.getpreferredencoding(False))'], env=environment).decode('ascii').strip()
        self.assertNotEqual(codecs.lookup(probe).name, 'utf-8')
        original = (ROOT / 'tests/ci/fixtures/tiny-skia-0.12.0-LICENSE').read_bytes()
        authors = ['Kornel Lesiński', 'Author α']
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            packages = []
            for name in ['tiny-skia', *sorted(notices.NATIVE_CRATES)]:
                source = tmp / name
                source.mkdir()
                (source / 'LICENSE').write_bytes(original)
                packages.append(dict(id=name, name=name, version='1', authors=authors,
                                     manifest_path=str(source / 'Cargo.toml'), license='BSD-3-Clause',
                                     license_file=None, source='test-registry'))
            metadata = tmp / 'metadata.json'
            metadata.write_text(json.dumps(dict(packages=packages, workspace_members=[]),
                                           ensure_ascii=False), encoding='utf-8')
            dav1d = tmp / 'dav1d-COPYING'
            dav1d.write_bytes(original)
            for kind in ['web', 'native']:
                with self.subTest(kind=kind, default_encoding=probe):
                    output = tmp / kind
                    command = [sys.executable, str(ROOT / 'scripts/ci/collect_notices.py'),
                               str(output), '--kind', kind, '--metadata', str(metadata)]
                    if kind == 'native':
                        command += ['--dav1d-license', str(dav1d)]
                    result = subprocess.run(command, env=environment, capture_output=True)
                    self.assertEqual(result.returncode, 0, result.stderr.decode('utf-8', errors='replace'))
                    inventory = json.loads((output / 'rust/inventory.json').read_bytes())
                    self.assertFalse(inventory['missing'])
                    self.assertTrue(all(p['authors'] == authors for p in inventory['packages']))
                    self.assertEqual((output / 'rust/tiny-skia-1/LICENSE').read_bytes(), original)

    def test_cargo_license_file_is_honored_and_missing_notice_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            source = tmp / 'source'
            source.mkdir()
            package = dict(id='example', name='example', version='1', source='test-registry',
                           manifest_path=str(source / 'Cargo.toml'), license=None,
                           license_file=str(source / 'terms.txt'), repository=None)
            metadata = dict(packages=[package], workspace_members=[])
            (source / 'terms.txt').write_bytes(b'Original custom license\r\n')
            notices.collect_dependencies(metadata, tmp / 'success')
            self.assertEqual((tmp / 'success/rust/example-1/terms.txt').read_bytes(),
                             b'Original custom license\r\n')
            (source / 'terms.txt').unlink()
            (source / 'LICENSE').write_text('A different notice cannot replace a missing declared license_file')
            with self.assertRaisesRegex(ValueError, 'example'):
                notices.collect_dependencies(metadata, tmp / 'missing-declared')
            (source / 'LICENSE').unlink()
            package.update(license_file=None, license='BSD-3-Clause')
            with self.assertRaisesRegex(ValueError, 'example'):
                notices.collect_dependencies(metadata, tmp / 'missing-text')
            inventory = json.loads((tmp / 'missing-text/rust/inventory.json').read_text())
            self.assertEqual(inventory['missing'][0]['name'], 'example')
            (source / 'LICENSE').write_bytes(b'')
            with self.assertRaises(ValueError):
                notices.collect_dependencies(metadata, tmp / 'empty-license')

    def test_supplement_requires_exact_package_revision_and_original_bytes(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            source = tmp / 'source'
            source.mkdir()
            vcs = source / '.cargo_vcs_info.json'
            vcs.write_text('{"git":{"sha1":"published-commit"}}')
            package = dict(id='example', name='example', version='1', source='test-registry',
                           manifest_path=str(source / 'Cargo.toml'), license='BSD-3-Clause',
                           license_file=None, repository='https://example.invalid/test')
            metadata = dict(packages=[package], workspace_members=[])
            supplements = tmp / 'supplements'
            supplements.mkdir()
            raw = (ROOT / 'tests/ci/fixtures/tiny-skia-0.12.0-LICENSE').read_bytes()
            blob = supplements / 'original.txt'
            blob.write_bytes(raw)
            notice = dict(file='original.txt', path='LICENSE', bytes=len(raw), role='license-terms',
                          sha256=hashlib.sha256(raw).hexdigest(),
                          git_blob_sha1=hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest(),
                          url='https://example.invalid/published-commit/LICENSE')
            catalog = dict(groups=[dict(package_commit='published-commit', basis='exact upstream blob',
                                       packages=[dict(name='example',version='1',license='BSD-3-Clause')],
                                       notices=[notice])])
            (supplements / 'manifest.json').write_text(json.dumps(catalog))
            notices.collect_dependencies(metadata, tmp / 'success', supplements)
            self.assertEqual((tmp / 'success/rust/example-1/upstream/LICENSE').read_bytes(), raw)
            notice['role'] = 'published-attribution'
            (supplements / 'manifest.json').write_text(json.dumps(catalog))
            with self.assertRaises(ValueError):
                notices.collect_dependencies(metadata, tmp / 'attribution-is-not-terms', supplements)
            notice['role'] = 'license-terms'
            (supplements / 'manifest.json').write_text(json.dumps(catalog))
            blob.write_bytes(raw + b'changed')
            with self.assertRaises(ValueError):
                notices.collect_dependencies(metadata, tmp / 'changed-bytes', supplements)
            blob.write_bytes(raw)
            vcs.write_text('{"git":{"sha1":"different-commit"}}')
            with self.assertRaises(ValueError):
                notices.collect_dependencies(metadata, tmp / 'changed-revision', supplements)
            vcs.write_text('{"git":{"sha1":"published-commit"}}')
            package['license'] = 'MIT'
            with self.assertRaises(ValueError):
                notices.collect_dependencies(metadata, tmp / 'changed-license', supplements)

    def test_native_mode_cannot_omit_system_dav1d_license(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(ValueError, 'dav1d'):
                notices.collect(ROOT, dict(packages=[], workspace_members=[]), Path(tmp), 'native')

    def test_r134_dispatch_exception_is_exact_and_labeled(self):
        catalog_root = ROOT / 'scripts/ci/license-supplements'
        catalog = json.loads((catalog_root / 'manifest.json').read_text())
        group = next(g for g in catalog['groups'] if g.get('ruling') == 'R134')
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            source = tmp / 'package'
            source.mkdir()
            for name in ['Cargo.toml.orig', 'README.md']:
                shutil.copyfile(catalog_root / 'dispatch-0.2.0' / name, source / name)
            # Preserve exact original VCS metadata identity from the published package.
            (source / '.cargo_vcs_info.json').write_text('{\n  "git": {\n    "sha1": "82d6c7a5b75dc0c71c3f46f87bb6c16a476f7748"\n  }\n}\n')
            package = dict(id='dispatch', name='dispatch', version='0.2.0', source=group['registry_source'],
                           manifest_path=str(source / 'Cargo.toml'), license='MIT', license_file=None, authors=['Steven Sheldon'])
            metadata = dict(packages=[package], workspace_members=[])
            notices.collect_dependencies(metadata, tmp / 'success')
            output = tmp / 'success/rust/dispatch-0.2.0/upstream'
            for name in ['Cargo.toml.orig', 'README.md']:
                self.assertEqual((output / name).read_bytes(), (source / name).read_bytes())
            terms = (output / 'SUPPLEMENTAL-MIT-TERMS.txt').read_text()
            self.assertIn('supplies no\noriginal license or copyright notice', terms)
            self.assertIn('Steven Sheldon', terms)
            self.assertIn('Permission is hereby granted', terms)
            self.assertNotIn('Copyright (c)', terms)
            self.assertIn('Original upstream license/copyright notice absent',
                          (tmp / 'success/rust/inventory.json').read_text())
            package['source'] = 'another-registry'
            with self.assertRaises(ValueError):
                notices.collect_dependencies(metadata, tmp / 'wrong-registry')
            package['source'] = group['registry_source']
            modified_catalog = tmp / 'wrong-checksum-catalog'
            shutil.copytree(catalog_root, modified_catalog)
            modified = json.loads((modified_catalog / 'manifest.json').read_text())
            next(g for g in modified['groups'] if g.get('ruling') == 'R134')['cargo_lock_checksum'] = '0' * 64
            (modified_catalog / 'manifest.json').write_text(json.dumps(modified))
            with self.assertRaises(ValueError):
                notices.collect_dependencies(metadata, tmp / 'changed-checksum', modified_catalog)
            (source / 'README.md').write_text('Changed attribution')
            with self.assertRaises(ValueError):
                notices.collect_dependencies(metadata, tmp / 'changed-original')

    def test_declared_license_cannot_copy_outside_package(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            source = tmp / 'source'
            source.mkdir()
            outside = tmp / 'private-not-a-license'
            outside.write_text('Unrelated file must not enter an artifact')
            package = dict(id='example', name='example', version='1', source='test-registry',
                           manifest_path=str(source / 'Cargo.toml'), license=None,
                           license_file=str(outside), repository=None)
            with self.assertRaises(ValueError):
                notices.collect_dependencies(dict(packages=[package], workspace_members=[]), tmp / 'output')
            self.assertFalse((tmp / 'output/rust/example-1/cargo-license-file/private-not-a-license').exists())

    def test_r141_wrapper_terms_are_guarded_and_vendor_notices_remain_distinct(self):
        catalog_root = ROOT / 'scripts/ci/license-supplements'
        group = next(g for g in json.loads((catalog_root / 'manifest.json').read_text())['groups']
                     if g.get('ruling') == 'R141')
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            source = tmp / 'source'
            shutil.copytree(catalog_root / 'published/libwebp-sys-0.9.6', source)
            package = dict(group['packages'][0], id='libwebp-sys', source=group['registry_source'],
                           manifest_path=str(source / 'Cargo.toml'), license_file=None)
            metadata = dict(packages=[package], workspace_members=[])
            notices.collect_dependencies(metadata, tmp / 'success')
            output = tmp / 'success/rust/libwebp-sys-0.9.6'
            for name in ['COPYING', 'AUTHORS']:
                self.assertEqual((output / 'vendor' / name).read_bytes(), (source / 'vendor' / name).read_bytes())
            for name in ['Cargo.toml.orig', 'README.md', '.cargo_vcs_info.json']:
                self.assertEqual((output / 'upstream' / name).read_bytes(), (source / name).read_bytes())
            terms = (output / 'upstream/SUPPLEMENTAL-MIT-TERMS.txt').read_text()
            self.assertIn('not supplied or recovered', terms)
            self.assertNotIn('Copyright (c)', terms)
            self.assertIn('Original wrapper license/copyright notice absent',
                          (tmp / 'success/rust/inventory.json').read_text())
            for field, wrong in [('source', 'wrong-registry'), ('authors', ['Wrong author']), ('license', 'BSD-3-Clause')]:
                previous = package[field]
                package[field] = wrong
                with self.assertRaises(ValueError):
                    notices.collect_dependencies(metadata, tmp / ('changed-' + field))
                package[field] = previous
            changed = tmp / 'changed-catalog'
            changed.mkdir()
            (changed / 'manifest.json').write_text(json.dumps(dict(groups=[dict(group, cargo_lock_checksum='0' * 64)])))
            with self.assertRaises(ValueError):
                notices.collect_dependencies(metadata, tmp / 'wrong-checksum', changed)
            (source / 'README.md').write_text('Changed original metadata')
            with self.assertRaises(ValueError):
                notices.collect_dependencies(metadata, tmp / 'changed-original')

    def test_r135_release_and_later_terms_have_narrow_registry_and_author_guards(self):
        catalog_root = ROOT / 'scripts/ci/license-supplements'
        catalog = json.loads((catalog_root / 'manifest.json').read_text())
        groups = [g for g in catalog['groups'] if g.get('ruling') == 'R135']
        self.assertEqual({p['name'] for g in groups for p in g['packages']},
                         {'ecolor', 'egui', 'egui-wgpu', 'egui-winit', 'egui_glow',
                          'emath', 'epaint', 'simd_helpers'})
        for group in groups:
            with tempfile.TemporaryDirectory() as tmp:
                tmp = Path(tmp)
                for notice in group['notices']:
                    if 'package_file' in notice:
                        shutil.copyfile(catalog_root / notice['file'], tmp / notice['package_file'])
                if group['packages'][0]['name'] == 'simd_helpers':
                    self.assertEqual(group['provenance']['changed_files'],
                                     [dict(path='LICENSE', before=None, after='1b7c6aaa5106ef1459ed8c539c06d640e0e0a784')])
                    self.assertFalse(group['provenance']['license_shipped_in_published_package'])
                else:
                    self.assertFalse(group['provenance']['exact_recovery_from_published_commit'])
                    self.assertIn('unresolved', group['basis'])
                # Use the exact published VCS bytes copied below from the reviewed metadata fixture.
                name = group['packages'][0]['name']
                fixture = catalog_root / 'published' / (name + '-' + group['packages'][0]['version']) / '.cargo_vcs_info.json'
                shutil.copyfile(fixture, tmp / '.cargo_vcs_info.json')
                package = dict(group['packages'][0], id=name, source='wrong-registry',
                               manifest_path=str(tmp / 'Cargo.toml'), license_file=None)
                with self.assertRaisesRegex(ValueError, 'R135 registry or Cargo.lock checksum changed'):
                    notices.supplemental_notices(package, catalog_root)
                package['source'] = group['registry_source']
                modified_catalog = tmp / 'wrong-checksum'
                modified_catalog.mkdir()
                altered = dict(group, cargo_lock_checksum='0' * 64)
                (modified_catalog / 'manifest.json').write_text(json.dumps(dict(groups=[altered])))
                with self.assertRaisesRegex(ValueError, 'R135 registry or Cargo.lock checksum changed'):
                    notices.supplemental_notices(package, modified_catalog)
                package['authors'] = ['Wrong author']
                with self.assertRaisesRegex(ValueError, 'published authors'):
                    notices.supplemental_notices(package, catalog_root)

if __name__ == '__main__':
    unittest.main()
