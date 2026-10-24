"""Pinned bundled-font hashes: every provenance entry with a bundled_path (R131 RTL faces, the
R183 follow-up script faces) must match its recorded size and SHA-256 and ship its licence."""
import hashlib
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class FontProvenanceTests(unittest.TestCase):
    def test_bundled_fonts_match_pinned_hashes(self):
        entries = []
        for provenance in sorted(ROOT.glob('assets/font*/*provenance.json')):
            fonts = json.loads(provenance.read_text()).get('fonts', [])
            entries += [f for f in fonts if 'bundled_path' in f]
        self.assertGreaterEqual(len(entries), 14)
        for entry in entries:
            path = ROOT / entry['bundled_path']
            raw = path.read_bytes()
            self.assertEqual(len(raw), entry['bytes'], path)
            self.assertEqual(hashlib.sha256(raw).hexdigest(), entry['sha256'], path)
            self.assertTrue((path.parent / 'LICENSE.txt').is_file(), path)


if __name__ == '__main__':
    unittest.main()
