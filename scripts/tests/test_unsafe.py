import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('unsafe_check', Path(__file__).resolve().parents[1] / 'check_unsafe.py')
check_unsafe = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_unsafe)


class UnsafeDebtTests(unittest.TestCase):
    def test_rust_unsafe_diagnostics_and_duplicate_build_targets_are_counted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'unsafe.rs').write_text('unsafe fn f() {}')
            diagnostic = json.dumps({'reason': 'compiler-message', 'message': {
                'code': {'code': 'E0133'},
                'spans': [{'file_name': 'unsafe.rs', 'is_primary': True,
                           'line_start': 1, 'column_start': 1}]}})
            found = check_unsafe.inventory([diagnostic, diagnostic, 'cargo progress'], root)
            self.assertEqual(found['unsafe.rs']['diagnostics'], {'unsafe_op_in_unsafe_fn': 1})

    def test_new_removed_or_changed_debt_requires_review(self):
        path = 'crates/capntproto-core/src/private/layout.rs'
        item = {'sha256': 'old-source', 'diagnostics': {'unsafe_op_in_unsafe_fn': 1}}
        baseline = {'files': {path: item}}
        self.assertEqual(check_unsafe.check({path: item}, baseline), [])
        for actual in ({}, {path: dict(item, sha256='changed-source')},
                       {path: dict(item, diagnostics={})},
                       {path: item, 'src/rpc/tls.rs': item}):
            self.assertTrue(check_unsafe.check(actual, baseline))

    def test_application_code_cannot_be_baselined(self):
        with self.assertRaisesRegex(ValueError, 'imported core'):
            check_unsafe.check({}, {'files': {'src/storage.rs': {}}})
