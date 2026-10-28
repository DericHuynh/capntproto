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
            self.assertEqual(report['aggregate']['tls']['legacy-baseline']['lower_minima'], 24)
            self.assertEqual(report['profiles'][0]['per_operation'], dict(blocks=10, bytes=160))
            self.assertEqual(len(report['profile_peers']), 5)
            self.assertEqual(len(report['unpaired_profile_peers']), 5)
            self.assertEqual(report['profile_failures'], failures)
            self.assertEqual(report['measurement_outcome'], 'failure')
            self.assertEqual(report['traces'][0]['counts']['read_positive']['mean'], 50)
            allocations = json.loads((output/'allocation-sites.json').read_text())
            self.assertEqual(len(allocations), 5)
            self.assertTrue(all(row['sites'][0]['blocks'] == 2 for row in allocations))


    def test_native_only_and_missing_diagnostics_preserve_validated_latency(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)/'secure'
            output = Path(directory)/'analysis'
            rows, trials = [], []
            for transport in ['tls', 'quic']:
                for case in range(24):
                    name = f'case-{case}'
                    variants = {}
                    labels = [('portable', 100), ('legacy', 200)]
                    if transport == 'tls':
                        labels += [('grpc', 300), ('websocket', 400)]
                    for label, latency in labels:
                        samples = [[latency + rep]*1000 for rep in range(5)]
                        variants[label] = summarize(samples)
                        for rep, values in enumerate(samples):
                            path = f'{transport}-{name}-{label}-{rep}'
                            trials.append(dict(transport=transport, case=name, client=label, path=path, mixed=False, repetition=rep))
                            save(root/'native'/path/'trial.json', dict(iterations=1000, latency_ns=values,
                                 security=dict(tls='1.3', authentication='server-certificate')))
                            save(root/'native'/path/'host.json', dict(before={}, after={}))
                    rows.append(dict(transport=transport, case=dict(name=name, family='workflow'), variants=variants))
            save(root/'native/summary.json', dict(diagnostic=False, rows=rows))
            scope = dict(diagnostic=False, repetitions=5, iterations=1000, comparison_version=1,
                 trials=trials, binaries={k:{} for k in ['portable', 'legacy', 'grpc', 'websocket']})
            save(root/'native/scope.json', scope)
            save(root.parent/'host.json', {})
            (root/'cpu.txt').write_text('synthetic analysis fixture')
            def analyze(destination, *flags):
                return subprocess.run([sys.executable, str(HERE/'analyze-secure-ci.py'), '1', str(destination),
                                       '--saved-root', str(root), '--measurement-outcome', 'success', *flags],
                                      env=dict(os.environ, GITHUB_SHA='fixture', GITHUB_REPOSITORY='fixture/repo'),
                                      capture_output=True, text=True)
            result = analyze(output, '--native-only')
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads((output/'summary.json').read_text())
            self.assertTrue(report['native_only'])
            self.assertFalse(report['diagnostics_requested'])
            self.assertFalse(report['diagnostics_complete'])
            self.assertEqual(report['native_trials'], 720)
            self.assertEqual(report['aggregate']['tls']['legacy']['lower_minima'], 24)
            self.assertEqual(report['aggregate']['tls']['grpc']['lower_minima'], 24)
            self.assertEqual(report['aggregate']['tls']['websocket']['lower_minima'], 24)
            self.assertNotIn('grpc', report['aggregate']['quic'])
            self.assertEqual(report['profile_trials'], 0)
            self.assertEqual(report['profiles'], [])
            self.assertEqual(report['traces'], [])
            self.assertNotIn('allocation_comparison', report)
            # Missing profiles still fail a full campaign, after salvaging latency.
            incomplete = Path(directory)/'incomplete'
            self.assertNotEqual(analyze(incomplete).returncode, 0)
            self.assertTrue((incomplete/'native-summary.json').is_file())
            self.assertFalse((incomplete/'summary.json').exists())
            # Omitting an external protocol or repeating a run cannot make a
            # partial comparison matrix look complete.
            comparator = rows[0]['variants'].pop('grpc')
            save(root/'native/summary.json', dict(diagnostic=False, rows=rows))
            self.assertNotEqual(analyze(Path(directory)/'missing-protocol', '--native-only').returncode, 0)
            rows[0]['variants']['grpc'] = comparator
            save(root/'native/summary.json', dict(diagnostic=False, rows=rows))
            trials[0]['repetition'] = 1
            save(root/'native/scope.json', scope)
            self.assertNotEqual(analyze(Path(directory)/'duplicate-run', '--native-only').returncode, 0)
            trials[0]['repetition'] = 0
            save(root/'native/scope.json', scope)
            # The fast path must validate raw samples, not trust the summary.
            first = root/'native'/trials[0]['path']/'trial.json'
            corrupted = json.loads(first.read_text())
            corrupted['latency_ns'][0] = 0
            save(first, corrupted)
            invalid = Path(directory)/'invalid'
            self.assertNotEqual(analyze(invalid, '--native-only').returncode, 0)
            self.assertFalse((invalid/'summary.json').exists())
            self.assertFalse((invalid/'native-summary.json').exists())

    def test_instruction_only_has_no_native_io_or_allocation_claims(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)/'secure'
            output=Path(directory)/'analysis'
            save(root.parent/'host.json', {})
            root.mkdir()
            (root/'cpu.txt').write_text('synthetic fixture')
            trials=[]
            services=[dict(hop=0),dict(hop=1)]
            for n in [5,50]:
                path=f'callgrind-{n}'
                trials.append(dict(path=path,mode='callgrind',transport='tls',case='fixture',implementation='cpp',iterations=n,points=[5,50],services=services))
                for peer in ['client','service-0','service-1']:
                    target=root/'profiles'/path/(peer+'.callgrind')
                    target.parent.mkdir(parents=True,exist_ok=True)
                    target.write_text(f'events: Ir Bcm Bim\nsummary: {100+10*n} {20+2*n} {5+n}\n')
            save(root/'profiles/scope.json',dict(instruction_only=True,diagnostic=True,binaries={'cpp':{}},trials=trials,failures=[]))
            subprocess.run([sys.executable,str(HERE/'analyze-secure-ci.py'),'1',str(output),'--saved-root',str(root),'--measurement-outcome','success'],env=dict(os.environ,GITHUB_SHA='fixture',GITHUB_REPOSITORY='fixture/repo'),stdout=subprocess.DEVNULL,check=True)
            report=json.loads((output/'summary.json').read_text())
            self.assertTrue(report['instruction_only'])
            self.assertFalse(report['allocation_only'])
            self.assertEqual(report['native_trials'],0)
            self.assertEqual(report['latency'],[])
            self.assertEqual(report['traces'],[])
            self.assertEqual(report['arena_counts'],[])
            self.assertNotIn('allocation_comparison',report)
            self.assertEqual(report['profile_trials'],2)
            self.assertEqual(len(report['profile_peers']),3)
            self.assertEqual(report['profiles'][0]['per_operation'],dict(Ir=30,Bcm=6,Bim=3))
            self.assertFalse((output/'native-summary.json').exists())

    def test_control_checkout_paths_have_the_same_instruction_categories(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)/'secure'
            output=Path(directory)/'analysis'
            save(root.parent/'host.json', {})
            root.mkdir()
            (root/'cpu.txt').write_text('synthetic fixture')
            tools=root.parent/'bin'
            tools.mkdir()
            annotate=tools/'callgrind_annotate'
            annotate.write_text(f'''#!{sys.executable}
import pathlib,sys
path=pathlib.Path(sys.argv[-1])
n=int(path.parent.name.rsplit('-',1)[1])
variant=path.parent.name.rsplit('-',1)[0]
prefix='target/'+variant.replace('-baseline','-baseline-source')+'/'
if variant=='legacy-baseline':prefix='/home/runner/work/capntproto/capntproto/'+prefix
print(30*n,6*n,'PROGRAM TOTALS')
for source in ['crates/capntproto-rpc/src/scoped/wire/coordinator/work.rs','crates/capntproto-rpc-wire/src/owned/connection/maintenance.rs','vendor/rustls/src/conn.rs']:
 print(10*n,2*n,prefix+source+':fixture')
''')
            annotate.chmod(0o755)
            trials=[]
            for variant in ['portable-baseline','legacy-baseline']:
                for n in [5,50]:
                    path=f'{variant}-{n}'
                    trials.append(dict(path=path,mode='callgrind',transport='tls',case='fixture',implementation=variant,iterations=n,points=[5,50],services=[]))
                    target=root/'profiles'/path/'client.callgrind'
                    target.parent.mkdir(parents=True,exist_ok=True)
                    target.write_text(f'events: Ir Bcm\nsummary: {30*n} {6*n}\n')
            save(root/'profiles/scope.json',dict(instruction_only=True,diagnostic=True,binaries={},trials=trials,failures=[]))
            subprocess.run([sys.executable,str(HERE/'analyze-secure-ci.py'),'1',str(output),'--saved-root',str(root),'--measurement-outcome','success'],env=dict(os.environ,GITHUB_SHA='fixture',GITHUB_REPOSITORY='fixture/repo',PATH=str(tools)+os.pathsep+os.environ['PATH']),stdout=subprocess.DEVNULL,check=True)
            report=json.loads((output/'summary.json').read_text())
            self.assertEqual(len(report['categories']),2)
            for row in report['categories']:
                self.assertEqual(row['categories'],dict.fromkeys(['portable_hosted','portable_wire','tls_crypto'],dict(Ir=10,Bcm=2)))

    def test_allocation_only_has_no_native_or_io_claims(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)/'secure'
            output=Path(directory)/'analysis'
            save(root.parent/'host.json', {})
            root.mkdir()
            (root/'cpu.txt').write_text('synthetic fixture')
            trials=[]
            for n in [5,50]:
                path=f'dhat-{n}'
                trials.append(dict(path=path,mode='dhat',transport='tls',case='fixture',implementation='legacy',iterations=n,points=[5,50],services=[]))
                save(root/'profiles'/path/'client.dhat.json',dict(ftbl=['site'],pps=[dict(tbk=2*n,tb=16*n,fs=[0])]))
            save(root/'profiles/scope.json',dict(allocation_only=True,diagnostic=True,binaries={'legacy':{}},trials=trials,failures=[],allocation_workloads=[dict(transport='tls',case='fixture',points=[5,50])]))
            subprocess.run([sys.executable,str(HERE/'analyze-secure-ci.py'),'1',str(output),'--saved-root',str(root),'--measurement-outcome','success'],env=dict(os.environ,GITHUB_SHA='fixture',GITHUB_REPOSITORY='fixture/repo'),stdout=subprocess.DEVNULL,check=True)
            report=json.loads((output/'summary.json').read_text())
            self.assertTrue(report['allocation_only'])
            self.assertFalse(report['allocation_comparison']['allocation_target_met'])
            self.assertEqual(report['allocation_comparison']['unmeasured'], 1)
            comparison = json.loads((output/'allocation-comparison.json').read_text())
            self.assertEqual(comparison['rows'][0]['missing'], ['portable', 'cpp'])
            self.assertEqual(report['native_trials'],0)
            self.assertEqual(report['latency'],[])
            self.assertEqual(report['traces'],[])
            self.assertEqual(report['profiles'][0]['per_operation'],dict(blocks=2,bytes=16))
            self.assertEqual(json.loads((output/'allocation-sites.json').read_text())[0]['sites'][0]['blocks'],2)

if __name__ == '__main__':
    unittest.main()
