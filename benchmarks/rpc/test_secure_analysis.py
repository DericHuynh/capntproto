"""Analyze synthetic saved evidence; never run or time a benchmark."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from latency_stats import summarize

HERE = Path(__file__).resolve().parent

def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value))

class SavedAnalysisTests(unittest.TestCase):
    def test_legacy_control_variable_points_and_all_service_peers(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'secure'
            output = Path(directory) / 'analysis'
            rows, trials = [], []
            services = [dict(hop=i) for i in range(4)]
            for transport in ['tls', 'quic']:
                for case in range(24):
                    name = f'case-{case}'
                    variants = {}
                    for label, latency in [('legacy', 100), ('legacy-baseline', 200)]:
                        samples = [[latency + rep] * 1000 for rep in range(5)]
                        variants[label] = summarize(samples)
                        for rep, values in enumerate(samples):
                            path = f'{transport}-{name}-{label}-{rep}'
                            trials.append(dict(transport=transport, case=name, client=label, path=path, mixed=False))
                            save(root/'native'/path/'trial.json', dict(iterations=1000, latency_ns=values,
                                 security=dict(tls='1.3', authentication='server-certificate')))
                            save(root/'native'/path/'host.json', dict(before={}, after={}))
                    rows.append(dict(transport=transport, case=dict(name=name), variants=variants))
            save(root/'native/summary.json', dict(diagnostic=False, rows=rows))
            save(root/'native/scope.json', dict(diagnostic=False, repetitions=5, iterations=1000,
                 trials=trials, binaries={'legacy': {}, 'legacy-baseline': {}}))
            save(root.parent/'host.json', {})
            (root/'cpu.txt').write_text('synthetic analysis fixture')
            profiles = []
            for iterations in [5, 50]:
                path = f'dhat-{iterations}'
                profiles.append(dict(path=path, mode='dhat', transport='tls', case='concurrent',
                                     implementation='legacy', iterations=iterations, points=[5, 50], services=services))
                for peer in ['client', 'service-0', 'service-1', 'service-2', 'service-3']:
                    save(root/'profiles'/path/(peer+'.dhat.json'), dict(ftbl=['site'],
                         pps=[dict(tbk=3+2*iterations, tb=48+32*iterations, fs=[0])]))
            # A failed short trial must not turn an unmatched long trial into
            # a marginal count or discard the other completed controls.
            unpaired = dict(profiles[-1], path='unpaired-long', case='failed-pair')
            profiles.append(unpaired)
            for peer in ['client', 'service-0', 'service-1', 'service-2', 'service-3']:
                save(root/'profiles/unpaired-long'/(peer+'.dhat.json'),
                     dict(ftbl=['site'], pps=[dict(tbk=103, tb=1648, fs=[0])]))
            failures = [dict(path='failed-short', error='fixture disconnect')]
            save(root/'profiles/scope.json', dict(trials=profiles, failures=failures))
            for iterations in [10, 100]:
                traces, journals = [], {}
                for rep in range(5):
                    path = f'trace-{rep}'
                    traces.append(dict(path=path, transport='tls', case='concurrent', client='legacy', services=services))
                    for peer in ['client', 'service-0', 'service-1', 'service-2', 'service-3']:
                        journals[path+'/'+peer+'.strace'] = dict(counts=dict(read_positive=iterations))
                save(root/f'trace-{iterations}/scope.json', dict(trials=traces))
                save(root/f'trace-{iterations}/ciphertext-counts.json', dict(journals=journals))
            subprocess.run([sys.executable, str(HERE/'analyze-secure-ci.py'), '1', str(output), '--saved-root', str(root), '--measurement-outcome', 'failure'],
                           env=dict(os.environ, GITHUB_SHA='fixture', GITHUB_REPOSITORY='fixture/repo'),
                           stdout=subprocess.DEVNULL, check=True)
            report = json.loads((output/'summary.json').read_text())
            self.assertEqual(report['aggregate']['tls']['legacy-baseline']['lower_means'], 24)
            self.assertEqual(report['profiles'][0]['per_operation'], dict(blocks=10, bytes=160))
            self.assertEqual(len(report['profile_peers']), 5)
            self.assertEqual(len(report['unpaired_profile_peers']), 5)
            self.assertEqual(report['profile_failures'], failures)
            self.assertEqual(report['measurement_outcome'], 'failure')
            self.assertEqual(report['traces'][0]['counts']['read_positive']['mean'], 50)
            allocations = json.loads((output/'allocation-sites.json').read_text())
            self.assertEqual(len(allocations), 5)
            self.assertTrue(all(row['sites'][0]['blocks'] == 2 for row in allocations))

if __name__ == '__main__':
    unittest.main()
