import math
import unittest

from latency_stats import summarize


class LatencyStatisticsTests(unittest.TestCase):
    def test_sample_and_between_run_spread_are_distinct(self):
        result = summarize([[1, 3], [5, 7]])
        self.assertEqual(result['estimator'], 'minimum')
        self.assertEqual(result['statistics_version'], 2)
        self.assertEqual(result['mean_ns'], 4)
        self.assertEqual(result['repetition_mean_ns'], [2, 6])
        self.assertEqual(result['repetition_mean_range_ns'], [2, 6])
        self.assertEqual((result['min_ns'], result['max_ns']), (1, 7))
        self.assertAlmostEqual(result['stddev_ns'], math.sqrt(20 / 3))
        self.assertAlmostEqual(result['repetition_mean_stddev_ns'], math.sqrt(8))

    def test_five_repetitions_preserve_extremes_and_all_means(self):
        result = summarize([[n] * 1000 for n in [10, 20, 30, 40, 50]])
        self.assertEqual(result['mean_ns'], 30)
        self.assertEqual(result['repetition_mean_ns'], [10, 20, 30, 40, 50])
        self.assertEqual(result['repetition_mean_range_ns'], [10, 50])
        self.assertAlmostEqual(result['repetition_mean_stddev_ns'], math.sqrt(250))
        self.assertEqual(summarize([[7]])['stddev_ns'], 0)
        self.assertEqual(summarize([[7]])['repetition_mean_stddev_ns'], 0)


if __name__ == '__main__':
    unittest.main()
