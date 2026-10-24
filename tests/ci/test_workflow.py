"""Keep Trunk's inherited child-Cargo toolchain independent of runner defaults."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class WorkflowTests(unittest.TestCase):
    def test_trunk_build_inherits_the_pinned_rust_toolchain(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        web = workflow.split('  build-web:\n', 1)[1].split('\n  vendor-heic:', 1)[0]
        self.assertIn("    env:\n      RUSTUP_TOOLCHAIN: '1.99.1'\n", web)
        self.assertIn('trunk build --release --locked', web)
        asan = workflow.split('  vendor-heic-asan:\n', 1)[1]
        self.assertNotIn('RUSTUP_TOOLCHAIN', asan)
        self.assertIn('cargo +nightly-2025-11-06 test', asan)


if __name__ == '__main__':
    unittest.main()
