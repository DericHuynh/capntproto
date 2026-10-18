"""Descriptive statistics; sample standard deviations use n - 1 (zero for n = 1)."""
import statistics


def summarize(trials):
    assert trials and all(trials)
    values = [n for trial in trials for n in trial]
    means = [statistics.mean(trial) for trial in trials]
    return dict(statistics_version=1, mean_ns=statistics.mean(values),
                stddev_ns=statistics.stdev(values) if len(values) > 1 else 0.0,
                min_ns=min(values), max_ns=max(values), repetition_mean_ns=means,
                repetition_mean_stddev_ns=statistics.stdev(means) if len(means) > 1 else 0.0,
                repetition_mean_range_ns=[min(means), max(means)])
