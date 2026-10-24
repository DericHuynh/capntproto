import unittest
from work_profile import callgrind, comparisons, slope


class WorkProfile(unittest.TestCase):
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
