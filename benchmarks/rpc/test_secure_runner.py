import copy
import contextlib
import io
import json
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent

def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, HERE / filename)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value

runner = module('secure_runner', 'secure-runner.py')
counts = module('ciphertext_counts', 'ciphertext-counts.py')

class SecurityEvidenceTests(unittest.TestCase):
    def test_profiler_keeps_legacy_and_portable_selection_distinct(self):
        profiler = module('secure_profile', 'secure-profile.py')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            campaign = object.__new__(profiler.Diagnostic)
            campaign.certs = root / 'certificates'
            campaign.binaries = {label: root / 'driver' for label in ['legacy', 'portable', 'portable-baseline', 'legacy-baseline']}
            for mode in ['callgrind', 'dhat']:
                campaign.mode = mode
                for label in campaign.binaries:
                    path = root / (mode + '-' + label)
                    path.mkdir()
                    with patch.object(profiler.subprocess, 'Popen') as launch:
                        campaign.launch(label, ['workflow-measure'], 'quic', path, 'client', 2)
                        command = launch.call_args.args[0]
                        env = launch.call_args.kwargs['env']
                        self.assertIn('--tool=' + mode, command)
                        self.assertEqual(env['CAPNTPROTO_BENCH_PORTABLE'], '0' if runner.engine(label) == 'legacy' else '1')
                        self.assertEqual(env['CAPNTPROTO_BENCH_TRANSPORT'], 'quic')
                        self.assertEqual(env['CAPNTPROTO_BENCH_CERTS'], str(campaign.certs))

    def test_profile_failure_preserves_later_controls_and_fails_campaign(self):
        profiler = module('secure_profile', 'secure-profile.py')
        with tempfile.TemporaryDirectory() as directory, contextlib.ExitStack() as stack:
            root = Path(directory)
            driver = root / 'driver'
            driver.touch()
            output = root / 'profiles'
            stack.enter_context(patch.object(profiler.sys, 'argv', ['secure-profile.py',
                '--rust', str(driver), '--cpp', str(driver), '--legacy-baseline', str(driver),
                '--output', str(output)]))
            stack.enter_context(patch.object(profiler.r, 'idle_host'))
            stack.enter_context(patch.object(profiler.r, 'physical_cpus', return_value=([0, 1], 'fixture')))
            stack.enter_context(patch.object(profiler.r.certificates, 'generate', return_value=root))
            stack.enter_context(patch.object(profiler.r.topology, 'digest', return_value='fixture'))
            campaign = stack.enter_context(patch.object(profiler, 'Diagnostic')).return_value
            calls = []
            def trial(*args):
                calls.append(args)
                if len(calls) == 1:
                    raise AssertionError('fixture disconnect')
                return dict(services=[dict(hop=0)])
            campaign.trial.side_effect = trial
            stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
            with self.assertRaisesRegex(SystemExit, '1 profile trials failed'):
                profiler.main()
            scope = json.loads((output / 'scope.json').read_text())
            self.assertEqual(len(calls), 128)
            self.assertEqual(len(scope['trials']), 127)
            self.assertEqual(len(scope['failures']), 1)
            self.assertIn('fixture disconnect', scope['failures'][0]['error'])
            self.assertEqual(scope['trials'][-1]['implementation'], 'legacy-baseline')

    def test_plaintext_downgrade_wrong_transport_and_partial_samples_are_rejected(self):
        case = dict(family='loopback', name='loopback-64', payload_bytes=64)
        valid = dict(measurement_version=2, protocol='tcp', payload_bytes=64,
                     security=runner.security('tls'), warmup=2, iterations=2,
                     latency_ns=[10,20], elapsed_ns=35)
        runner.validate(valid, case, 'tls', 'portable', 2, 2)
        for field, value in [('security', None), ('security', runner.security('quic')),
                             ('latency_ns',[10]), ('elapsed_ns',29), ('protocol','capnp-cpp')]:
            invalid = copy.deepcopy(valid)
            invalid[field] = value
            with self.assertRaises(AssertionError):
                runner.validate(invalid, case, 'tls', 'portable', 2, 2)
        for field, value in [('tls','1.2'),('early_data',True),('authentication','none')]:
            invalid = copy.deepcopy(valid)
            invalid['security'][field] = value
            with self.assertRaises(AssertionError):
                runner.validate(invalid, case, 'tls', 'portable', 2, 2)

    def test_campaign_lock_rejects_overlap_and_releases_after_close(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'lock'
            with runner.campaign_lock(path):
                with self.assertRaisesRegex(RuntimeError, 'another encrypted benchmark'):
                    runner.campaign_lock(path)
            with runner.campaign_lock(path):
                pass

    def test_frozen_portable_control_keeps_engine_and_transport_identity(self):
        case = dict(family='workflow', name='unary-small', payload_bytes=64)
        data = dict(case={'name':'unary-small','payload_bytes':64}, workflow_version=3,
                    implementation='portable', security=runner.security('quic'),
                    warmup=2, iterations=2, latency_ns=[10,20], elapsed_ns=35)
        runner.validate(data, case, 'quic', 'portable-baseline', 2, 2)
        self.assertTrue(runner.supported('portable-baseline', 'quic', case))
        data['implementation'] = 'legacy'
        with self.assertRaises(AssertionError):
            runner.validate(data, case, 'quic', 'portable-baseline', 2, 2)

    def test_frozen_legacy_control_keeps_engine_and_transport_identity(self):
        case = dict(family='topology', name='concurrent-32', depth=1, width=32)
        data = dict(case={k:v for k,v in case.items() if k != 'family'}, topology_version=1,
                    implementation='legacy', security=runner.security('quic'),
                    warmup=2, iterations=2, latency_ns=[10,20], elapsed_ns=35)
        runner.validate(data, case, 'quic', 'legacy-baseline', 2, 2)
        self.assertTrue(runner.supported('legacy-baseline', 'quic', case))
        data['implementation'] = 'portable'
        with self.assertRaises(AssertionError):
            runner.validate(data, case, 'quic', 'legacy-baseline', 2, 2)

    def test_ciphertext_counters_pair_resumed_calls_and_separate_datagrams_from_bytes(self):
        trace = '10 1.000 read(3<TCP:[127.0.0.1:1->127.0.0.1:2]>, <unfinished ...>\n11 1.001 write(1</tmp/file>, "abc", 3) = 3 <0.001>\n11 1.002 sendmmsg(4<UDP:[127.0.0.1:3]>, [], 2, 0) = 2 <0.001>\n10 1.003 <... read resumed>"abc", 3) = 3 <0.003>\n10 1.004 recvfrom(4<UDP:[127.0.0.1:3]>, 0x1, 1200, 0, NULL, NULL) = -1 EAGAIN (Resource temporarily unavailable) <0.001>\n10 1.005 write(3<TCP:[127.0.0.1:1->127.0.0.1:2]>, "data", 4) = 4 <0.001>\n'
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'trace'
            path.write_text(trace)
            result = counts.count(path)
        self.assertEqual(result['unfinished_socket_calls'], 0)
        self.assertEqual(result['counts']['read_attempts'], 2)
        self.assertEqual(result['counts']['read_positive'], 1)
        self.assertEqual(result['counts']['read_returned_bytes'], 3)
        self.assertEqual(result['counts']['write_attempts'], 2)
        self.assertEqual(result['counts']['write_returned_bytes'], 4)
        self.assertEqual(result['counts']['write_batched_datagrams'], 2)

    def test_termination_keeps_unreturned_socket_attempts_visible(self):
        trace = '10 1.000 read(3<TCP:[127.0.0.1:1->127.0.0.1:2]>, 0x1, 16) = ? ERESTARTSYS\n11 1.001 recvfrom(4<UDP:[127.0.0.1:3]>, <unfinished ...>\n'
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'trace'
            path.write_text(trace)
            result = counts.count(path)
        self.assertEqual(result['counts']['read_attempts'], 2)
        self.assertEqual(result['counts']['read_unreturned'], 2)
        self.assertEqual(result['unfinished_socket_calls'], 1)

if __name__ == '__main__':
    unittest.main()
