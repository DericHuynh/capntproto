import unittest
import importlib.util
import copy
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from work_profile import callgrind, comparisons, slope


class WorkProfile(unittest.TestCase):
    def test_legacy_gate_rejects_each_counter_without_rounding_or_averaging_cases(self):
        spec = importlib.util.spec_from_file_location('legacy_work_profile', Path(__file__).with_name('legacy-work-profile.py'))
        profile = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(profile)
        result = dict(comparisons=[dict(mode='callgrind', transport='tls', baseline='factored',
            candidate='candidate', case=case, repetitions=5,
            metrics={metric: {side: dict(mean=100, samples=[100]*5)
                              for side in ['baseline', 'candidate']}
                     for metric in ['Ir', 'Bcm', 'Bim']}) for case in profile.CASES])
        self.assertTrue(profile.regression_gate(result)['passed'])
        for metric in ['Ir', 'Bcm', 'Bim']:
            changed = copy.deepcopy(result)
            changed['comparisons'][0]['metrics'][metric]['candidate'] = dict(mean=100.00001, samples=[100.00001]*5)
            changed['comparisons'][1]['metrics'][metric]['candidate'] = dict(mean=50, samples=[50]*5)
            verdict = profile.regression_gate(changed)
            self.assertFalse(verdict['passed'])
            self.assertEqual([(r['case'], r['metric']) for r in verdict['failures']], [(profile.CASES[0], metric)])
        for invalid in ['missing', 'duplicate', 'repetitions', 'metric', 'nan']:
            changed = copy.deepcopy(result)
            if invalid == 'missing':
                changed['comparisons'].pop()
            elif invalid == 'duplicate':
                changed['comparisons'][-1] = changed['comparisons'][0]
            elif invalid == 'repetitions':
                changed['comparisons'][0]['repetitions'] = 4
            elif invalid == 'metric':
                del changed['comparisons'][0]['metrics']['Bim']
            else:
                changed['comparisons'][0]['metrics']['Ir']['candidate']['mean'] = float('nan')
            with self.assertRaises((AssertionError, KeyError)):
                profile.regression_gate(changed)

    def test_full_summary_keeps_all_repetitions_peers_and_identical_control(self):
        self.check_full_summary(pair_only=False)

    def test_pair_only_summary_keeps_all_repetitions_and_peers(self):
        self.check_full_summary(pair_only=True)

    def check_full_summary(self, pair_only):
        spec = importlib.util.spec_from_file_location('legacy_work_profile', Path(__file__).with_name('legacy-work-profile.py'))
        profile = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(profile)
        with TemporaryDirectory() as directory:
            root = Path(directory)
            labels = ['factored', 'candidate'] if pair_only else ['frozen', 'factored', 'repeat', 'candidate']
            scope = dict(repetitions=5, points=[10, 50], binaries=dict.fromkeys(labels), trials=[], failures=[])
            if pair_only:
                scope['pairs'] = [('factored', 'candidate')]
            for mode in ['callgrind', 'dhat']:
                for transport in (['tls'] if mode == 'callgrind' else ['tls', 'quic']):
                    for case in (profile.CASES if mode == 'callgrind' else profile.CASES[:3]):
                        for repetition in range(5):
                            for variant in labels:
                                for iterations in [10, 50]:
                                    path = f'{mode}-{transport}-{case}-{repetition}-{variant}-{iterations}'
                                    (root / path).mkdir()
                                    scope['trials'].append(dict(path=path, mode=mode, transport=transport,
                                                               case=case, repetition=repetition, variant=variant,
                                                               iterations=iterations, services=[dict(hop=0)]))
                                    for peer in ['client', 'service-0']:
                                        if mode == 'callgrind':
                                            rate = 10 + repetition - int(variant == 'candidate')
                                            # Unequal peers ensure the summary cannot silently keep only the client.
                                            multiplier = 1 if peer == 'client' else 2
                                            counts = ' '.join(str(100 + multiplier * value * iterations)
                                                              for value in [rate, 2, 1, 1, 1])
                                            (root / path / (peer + '.callgrind')).write_text(
                                                'positions: line\nevents: Ir Bc Bcm Bi Bim\nsummary: ' + counts
                                                + '\nfn=(1) work\n1 ' + counts + '\ntotals: ' + counts + '\n')
                                        else:
                                            (root / path / (peer + '.dhat.json')).write_text(json.dumps(
                                                dict(pps=[dict(tbk=17 + iterations * 3, tb=1000 + iterations * 64)])))
            (root / 'scope.json').write_text(json.dumps(scope))
            profile.summarize(root)
            result = json.loads((root / 'comparison.json').read_text())
            self.assertEqual(len(result['rows']), 120 if pair_only else 240)
            self.assertEqual(len(result['comparisons']), 12 if pair_only else 36)
            for row in result['comparisons']:
                self.assertEqual(row['repetitions'], 5)
                metric = row['metrics']['Ir' if row['mode'] == 'callgrind' else 'blocks']
                expected = -3 if row['mode'] == 'callgrind' and row['candidate'] == 'candidate' else 0
                self.assertEqual(metric['paired_delta']['samples'], [expected] * 5)
                if row['mode'] == 'callgrind':
                    self.assertEqual(metric['baseline']['samples'], [30, 33, 36, 39, 42])
                    self.assertEqual(metric['baseline']['mean'], 36)
            self.assertEqual(len(json.loads((root / 'functions.json').read_text())), 30 if pair_only else 90)
            self.assertEqual('exact factored executable' in (root / 'comparison.md').read_text(), not pair_only)

    def test_callgrind_excludes_call_arcs_and_fills_missing_zero_events(self):
        text = '''positions: line
events: Ir Bc Bcm Bi Bim
summary: 31 5 2 4 3
fn=(1) caller
1 10 2 1
cfn=(2) callee
calls=1 2
1 500 4 3 2 1
2 1 1 0 1 1
fn=(2)
3 20 2 1 3 1
totals: 31 5 2 4 2
'''
        summary, functions = callgrind(text)
        self.assertEqual(functions['caller']['Bim'], 1)
        self.assertEqual(functions['callee']['Ir'], 20)
        self.assertEqual(functions['[summary minus attributed total]']['Bim'], 1)
        self.assertEqual(summary['Bim'], 3)
        with self.assertRaises(AssertionError):
            callgrind(text.replace('totals: 31', 'totals: 32'))

    def test_negative_slopes_and_paired_spread_are_preserved(self):
        self.assertEqual(slope({'Ir': 90}, {'Ir': 50}, [10, 50]), {'Ir': -1})
        rows = [dict(mode='callgrind', transport='tls', case='case', variant=label,
                     repetition=i, counts={'Ir': value})
                for label, values in [('before', [10, 30]), ('after', [12, 28])]
                for i, value in enumerate(values)]
        result = comparisons(rows, [('before', 'after')])[0]['metrics']['Ir']
        self.assertEqual(result['paired_delta']['samples'], [2, -2])
        self.assertEqual(result['change_percent'], 0)
        self.assertAlmostEqual(result['paired_delta']['sd'], 2 ** 1.5)
        with self.assertRaises(KeyError):
            comparisons(rows[:-1], [('before', 'after')])
        with self.assertRaises(AssertionError):
            comparisons([*rows, rows[0]], [('before', 'after')])


if __name__ == '__main__':
    unittest.main()
