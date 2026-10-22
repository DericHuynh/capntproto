import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest

HERE = Path(__file__).resolve().parent

def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, HERE / filename)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value

runner = module('secure_runner', 'secure-runner.py')
counts = module('ciphertext_counts', 'ciphertext-counts.py')

class SecurityEvidenceTests(unittest.TestCase):
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
