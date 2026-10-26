//! Minimum latency estimates and descriptive statistics shared by runners and validators.
//! Call only after validating every trial and its equal sampling budget.
use serde_json::{json, Value};

pub fn summarize(trials: &[Vec<u64>]) -> Value {
    assert!(!trials.is_empty() && trials.iter().all(|trial| !trial.is_empty()));
    let samples = trials.iter().map(Vec::len).sum::<usize>();
    let total: u128 = trials.iter().flatten().map(|&n| u128::from(n)).sum();
    let mean = total as f64 / samples as f64;
    let variance = trials
        .iter()
        .flatten()
        .map(|&n| (n as f64 - mean).powi(2))
        .sum::<f64>()
        / samples.saturating_sub(1).max(1) as f64;
    let run_means: Vec<f64> = trials
        .iter()
        .map(|trial| trial.iter().map(|&n| u128::from(n)).sum::<u128>() as f64 / trial.len() as f64)
        .collect();
    let run_mean = run_means.iter().sum::<f64>() / run_means.len() as f64;
    let run_variance = run_means
        .iter()
        .map(|n| (n - run_mean).powi(2))
        .sum::<f64>()
        / run_means.len().saturating_sub(1).max(1) as f64;
    json!({
        "statistics_version": 2,
        "estimator": "minimum",
        "mean_ns": mean,
        "stddev_ns": variance.sqrt(),
        "min_ns": trials.iter().flatten().min().unwrap(),
        "max_ns": trials.iter().flatten().max().unwrap(),
        "repetition_mean_ns": run_means,
        "repetition_mean_stddev_ns": run_variance.sqrt(),
        "repetition_mean_range_ns": [
            run_means.iter().copied().fold(f64::INFINITY, f64::min),
            run_means.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_sample_spread_from_between_run_spread() {
        let value = summarize(&[vec![1, 3], vec![5, 7]]);
        assert_eq!(value["mean_ns"], 4.0);
        assert_eq!(value["estimator"], "minimum");
        assert_eq!(value["min_ns"], 1);
        assert_eq!(value["max_ns"], 7);
        assert_eq!(value["repetition_mean_ns"], json!([2.0, 6.0]));
        assert_eq!(value["repetition_mean_range_ns"], json!([2.0, 6.0]));
        assert!((value["stddev_ns"].as_f64().unwrap() - (20.0f64 / 3.0).sqrt()).abs() < 1e-12);
        assert!(
            (value["repetition_mean_stddev_ns"].as_f64().unwrap() - 8.0f64.sqrt()).abs() < 1e-12
        );
    }

    #[test]
    fn singleton_and_large_constant_samples_are_finite() {
        for trials in [vec![vec![7]], vec![vec![u64::MAX; 1000]; 5]] {
            let value = summarize(&trials);
            assert_eq!(value["mean_ns"].as_f64().unwrap(), trials[0][0] as f64);
            assert_eq!(value["stddev_ns"], 0.0);
            assert_eq!(value["repetition_mean_stddev_ns"], 0.0);
        }
    }
}
