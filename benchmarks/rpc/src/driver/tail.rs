//! Longer before/after trials with independent baselines and untrimmed tails.
use super::*;

const ITERATIONS: usize = 50_000;
const REPETITIONS: usize = 5;
const PEERS: [(&str, &str); 4] = [
    ("before", "tcp"),
    ("after", "tcp"),
    ("upstream", "capnp-rust"),
    ("cpp", "capnp-cpp"),
];

/// Descriptive tail comparison; no noisy wall-clock threshold gates correctness.
pub fn compare(executables: [&Path; 4], destination: &Path) -> Result<()> {
    if cfg!(debug_assertions) {
        return Err("tail comparisons require a release driver".into());
    }
    fs::create_dir(destination)?;
    let placement = Placement::discover()?;
    let identities = executables
        .iter()
        .zip(PEERS)
        .map(|(path, (variant, protocol))| {
            let digest = output(
                "sha256sum",
                &[path.to_str().ok_or("invalid executable path")?],
            )?;
            Ok(
                json!({"variant":variant,"protocol":protocol,"executable":path,
                "sha256":digest.split_whitespace().next().ok_or("missing digest")?}),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    fs::write(
        destination.join("environment.json"),
        serde_json::to_vec_pretty(&json!({
            "measurement_version":crate::MEASUREMENT_VERSION,
            "tcp_driver":crate::tcp_driver()?, "peers":identities,
            "cpu_placement":placement, "cpu":output("lscpu", &[])?,
            "clock_reads":clock_diagnostics(&placement)?,
            "warmup":10000, "iterations":ITERATIONS, "repetitions":REPETITIONS,
            "scope":"Plaintext TCP loopback, separate processes and physical cores. Rotated and alternating peer order. All samples retained, including stalls. Compare per-trial p99 ranges as well as pooled percentiles; local results do not qualify remote performance."
        }))?,
    )?;
    let mut trials = Vec::new();
    for repetition in 0..REPETITIONS {
        let mut order: Vec<_> = (0..PEERS.len()).collect();
        order.rotate_left(repetition % PEERS.len());
        if repetition % 2 != 0 {
            order.reverse();
        }
        for bytes in PAYLOADS {
            for &index in &order {
                let (variant, protocol) = PEERS[index];
                eprintln!("{variant}: {bytes} bytes, repetition {}", repetition + 1);
                let mut trial = trial_with_iterations(
                    executables[index],
                    protocol,
                    bytes,
                    Some(&placement),
                    ITERATIONS,
                )?;
                trial["variant"] = json!(variant);
                trial["repetition"] = json!(repetition);
                trials.push(trial);
                fs::write(
                    destination.join("trials.json"),
                    serde_json::to_vec(&trials)?,
                )?;
            }
        }
    }
    let summary = summarize(&trials, ITERATIONS)?;
    fs::write(
        destination.join("summary.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn summarize(trials: &[Value], iterations: usize) -> Result<Value> {
    if trials.len() != PEERS.len() * PAYLOADS.len() * REPETITIONS {
        return Err("incomplete tail comparison".into());
    }
    let percentile = |samples: &[u64], p: usize| samples[(samples.len() * p).div_ceil(100) - 1];
    let mut rows = Vec::new();
    for bytes in PAYLOADS {
        let mut peers = Vec::new();
        for (variant, protocol) in PEERS {
            let mut samples = Vec::new();
            let mut sample_trials = Vec::new();
            let mut tails = Vec::new();
            let mut elapsed = 0u128;
            for repetition in 0..REPETITIONS {
                let mut matching = trials.iter().filter(|t| {
                    t["variant"] == variant
                        && t["payload_bytes"] == bytes
                        && t["repetition"] == repetition
                });
                let trial = matching.next().ok_or("missing tail comparison cell")?;
                if matching.next().is_some() {
                    return Err("duplicate tail comparison cell".into());
                }
                validate_trial_with_iterations(trial, protocol, bytes, iterations)?;
                let mut values: Vec<_> = trial["latency_ns"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_u64().unwrap())
                    .collect();
                values.sort_unstable();
                tails.push(percentile(&values, 99));
                samples.extend_from_slice(&values);
                sample_trials.push(values);
                elapsed += u128::from(trial["elapsed_ns"].as_u64().unwrap());
            }
            samples.sort_unstable();
            let [p50, p95, p99] = [50, 95, 99].map(|p| percentile(&samples, p));
            let min = *tails.iter().min().unwrap();
            let max = *tails.iter().max().unwrap();
            let mut row = json!({"variant":variant,"protocol":protocol,
                "p50_ns":p50,"p95_ns":p95,"p99_ns":p99,
                "p99_over_p50":p99 as f64 / p50 as f64,
                "trial_p99_ns":tails,"p99_range_ns":[min,max],
                "samples":samples.len(),
                "sequential_requests_per_second":samples.len() as f64 * 1e9 / elapsed as f64});
            row.as_object_mut().unwrap().extend(
                latency_summary::summarize(&sample_trials)
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            peers.push(row);
        }
        let ratio = |numerator: usize, denominator: usize, metric: &str| {
            peers[numerator][metric].as_f64().unwrap()
                / peers[denominator][metric].as_f64().unwrap()
        };
        rows.push(json!({"payload_bytes":bytes,
            "after_before_p50":ratio(1,0,"p50_ns"),
            "after_before_p99":ratio(1,0,"p99_ns"),
            "after_before_throughput":ratio(1,0,"sequential_requests_per_second"),
            "after_upstream_p99":ratio(1,2,"p99_ns"),
            "after_cpp_p99":ratio(1,3,"p99_ns"),
            "peers":peers}));
    }
    Ok(json!({"iterations_per_trial":iterations,"repetitions":REPETITIONS,"rows":rows}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<Value> {
        let mut trials = Vec::new();
        for bytes in PAYLOADS {
            for (variant, protocol) in PEERS {
                for repetition in 0..REPETITIONS {
                    let mut samples = vec![100; 200];
                    if variant == "after" {
                        samples[195..].fill(1000 + repetition as u64 * 100);
                    }
                    trials.push(json!({"measurement_version":crate::MEASUREMENT_VERSION,
                        "variant":variant,"protocol":protocol,"payload_bytes":bytes,
                        "repetition":repetition,"warmup":10000,"iterations":200,
                        "elapsed_ns":samples.iter().sum::<u64>(),"latency_ns":samples}));
                }
            }
        }
        trials
    }

    #[test]
    fn tail_report_retains_stalls_and_distinguishes_trial_variation_from_pooled_tail() {
        let report = summarize(&fixture(), 200).unwrap();
        for row in report["rows"].as_array().unwrap() {
            let after = &row["peers"][1];
            assert_eq!(after["p50_ns"], 100);
            assert_eq!(after["p99_ns"], 1200);
            assert_eq!(after["p99_over_p50"], 12.0);
            assert_eq!(after["trial_p99_ns"], json!([1000, 1100, 1200, 1300, 1400]));
            assert_eq!(after["p99_range_ns"], json!([1000, 1400]));
            assert_eq!(row["after_before_p50"], 1.0);
            assert_eq!(row["after_before_p99"], 12.0);
            assert!(row["after_before_throughput"].as_f64().unwrap() < 1.0);
        }
    }

    #[test]
    fn tail_report_rejects_missing_duplicate_misidentified_and_malformed_trials() {
        let trials = fixture();
        assert!(summarize(&trials[..trials.len() - 1], 200).is_err());
        let mut duplicate = trials.clone();
        duplicate[1] = duplicate[0].clone();
        assert!(summarize(&duplicate, 200).is_err());
        for (key, value) in [
            ("variant", json!("unknown")),
            ("repetition", json!(5)),
            ("protocol", json!("grpc")),
            ("iterations", json!(199)),
            ("elapsed_ns", json!(1)),
            ("latency_ns", json!(vec![0; 200])),
        ] {
            let mut invalid = trials.clone();
            invalid[0][key] = value;
            assert!(summarize(&invalid, 200).is_err(), "accepted {key}");
        }
        assert!(summarize(&trials, 0).is_err());
    }
}
