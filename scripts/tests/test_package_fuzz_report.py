"""Raw AFL queue/crash names must survive portable artifact upload intact."""
import importlib.util
from pathlib import Path
import tarfile
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    'package_fuzz_report', Path(__file__).parents[1] / 'package_fuzz_report.py')
packager = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(packager)


class PackageTests(unittest.TestCase):
    def test_raw_findings_and_reports_round_trip_without_renaming(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            files = {
                'target/quality/fuzz/afl/capnp_framing/default/queue/id:000000,time:0,execs:0,orig:seed-1': b'seed',
                'target/quality/fuzz/afl/rpc_lifecycle/default/crashes/id:000001,sig:06': b'crash',
                'target/quality/fuzz/afl/rpc_lifecycle/default/hangs/id:000002,time:42': b'hang',
                'target/quality/fuzz/README.md': b'report',
                'target/quality/fuzz/FAILED-TESTS.md': b'diagnostics',
                'target/quality/fuzz/fuzz.svg': b'graph',
                'target/verification/native-fuzz/check/campaign.log': b'log',
                'fuzz/artifacts/native_packet/crash-input': bytes(range(256)),
            }
            for relative, content in files.items():
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(content)
            destination = packager.package(root)
            self.assertEqual(destination.name, 'fuzz-report.tar.gz')
            with tarfile.open(destination, 'r:gz') as archive:
                actual = {entry.name: archive.extractfile(entry).read()
                          for entry in archive if entry.isfile()}
            self.assertEqual(actual, files)

    def test_early_failure_missing_directories_and_reruns(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'target/quality/fuzz'
            source.mkdir(parents=True)
            evidence = source / 'evidence.json'
            evidence.write_text('{"passed":false}')
            destination = packager.package(root)
            with tarfile.open(destination) as archive:
                self.assertEqual(archive.extractfile('target/quality/fuzz/evidence.json').read(),
                                 b'{"passed":false}')
            evidence.unlink()
            source.rmdir()
            packager.package(root)
            with tarfile.open(destination) as archive:
                self.assertEqual(archive.getnames(), [])


if __name__ == '__main__':
    unittest.main()
