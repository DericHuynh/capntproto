"""Exercise finding/error handling independently of long mutation campaigns."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('afl_fuzz', Path(__file__).parents[1] / 'afl_fuzz.py')
afl = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(afl)


class AflTests(unittest.TestCase):
    def campaign(self, crashes=0, hangs=0, handshake=True, exit_code=0, stats=True):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            def execute(command, log, env, timeout):
                log.write_text('Using IJON feature.\n' if handshake else 'ordinary instrumentation\n')
                destination = root / 'out/rpc_lifecycle/default'
                destination.mkdir(parents=True)
                if stats:
                    (destination / 'fuzzer_stats').write_text(
                        f'execs_done : 100\ncorpus_count : 2\nsaved_crashes : {crashes}\n'
                        f'saved_hangs : {hangs}\nedges_found : 30\nstability : 99.1%\nbitmap_cvg : 20%\n')
                return exit_code

            with patch.object(afl, 'TARGETS', ('rpc_lifecycle',)), \
                 patch.object(afl, 'execute', side_effect=execute), \
                 patch.object(afl.subprocess, 'check_output', return_value='cargo-afl 0.18.2 (AFL++ version 4.40c)'):
                passed = afl.run(root / 'out', root / 'bin', root / 'corpus', 1)
            report = json.loads((root / 'out/afl.json').read_text())
            self.assertEqual(passed, report['passed'])
            return report

    def test_zero_exit_does_not_hide_crashes_or_hangs(self):
        self.assertTrue(self.campaign()['passed'])
        for kwargs in ({'crashes': 1}, {'hangs': 1}, {'handshake': False},
                       {'exit_code': 1}, {'stats': False}):
            with self.subTest(kwargs=kwargs):
                report = self.campaign(**kwargs)
                self.assertFalse(report['passed'])
                self.assertIsNotNone(report['campaigns'][0]['error'])

    def test_no_executions_cannot_be_reported_as_success(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'stats'
            path.write_text('execs_done : 0\ncorpus_count : 1\nsaved_crashes : 0\n'
                            'saved_hangs : 0\nedges_found : 0\nstability : 100%\nbitmap_cvg : 0%\n')
            with self.assertRaises(ValueError):
                afl.statistics(path)


if __name__ == '__main__':
    unittest.main()
