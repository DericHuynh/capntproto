"""Keep CI evidence distinct and propagate nextest failures."""
import importlib.util
from pathlib import Path
import tempfile
import tomllib
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('nextest_ci', Path(__file__).parents[1] / 'nextest_ci.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class NextestCI(unittest.TestCase):
    def test_each_invocation_has_its_own_report_and_preserves_failures(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / '.config').mkdir()
            (root / '.config/nextest.toml').write_text('[profile.ci.junit]\npath="junit.xml"\n')
            paths = []

            def execute(command, cwd):
                self.assertEqual(cwd, root)
                config = Path(command[command.index('--config-file') + 1])
                data = tomllib.loads(config.read_text())
                report = Path(data['profile']['evidence']['junit']['path'])
                self.assertFalse(report.exists())
                report.write_text('<testsuites/>')
                paths.append(report)
                return 100

            with patch.object(MODULE, '__file__', str(root / 'scripts/nextest_ci.py')), patch.object(MODULE.subprocess, 'call', side_effect=execute):
                for case in ['one', 'two', 'one']:
                    self.assertEqual(MODULE.run(['cargo', '--', '--test', case]), 100)
            self.assertNotEqual(paths[0], paths[1])
            self.assertEqual(paths[0], paths[2])
            self.assertTrue(all(p.exists() for p in paths))

    def test_success_without_junit_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / '.config').mkdir()
            (root / '.config/nextest.toml').write_text('')
            with patch.object(MODULE, '__file__', str(root / 'scripts/nextest_ci.py')), patch.object(MODULE.subprocess, 'call', return_value=0):
                with self.assertRaisesRegex(RuntimeError, 'no JUnit'):
                    MODULE.run(['cargo', '+nightly', 'careful', '--', '--lib'])
