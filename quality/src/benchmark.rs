//! Parsers for archived cross-implementation benchmark evidence. New profiles use runtime-runner.
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[path = "../../benchmarks/rpc/latency-summary.rs"]
mod latency_summary;
pub const PROTOCOLS: [&str; 6] = [
    "tcp",
    "capnp-cpp",
    "grpc",
    "websocket",
    "native",
    "capnp-rust",
];
pub const PAYLOADS: [usize; 4] = [0, 64, 1024, 65536];
const REPETITIONS: usize = 5;
pub const MEASUREMENT_VERSION: u32 = 2;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trial {
    pub measurement_version: u32,
    pub security: Value,
    pub protocol: String,
    pub payload_bytes: usize,
    pub warmup: u64,
    pub iterations: u64,
    pub elapsed_ns: u64,
    pub latency_ns: Vec<u64>,
}
pub fn validate(
    trial: &Trial,
    protocol: &str,
    bytes: usize,
    warmup: u64,
    iterations: u64,
) -> Result<()> {
    if trial.measurement_version != MEASUREMENT_VERSION {
        return Err("unsupported benchmark measurement version; rerun with matched C++ validation and cleanup".into());
    }
    let security = &trial.security;
    let encrypted = if protocol == "native" {
        security["transport"] == "native" && security["authentication"] == "pinned-mutual-identity"
    } else {
        security["transport"] == "tls-tcp"
            && security["tls"] == "1.3"
            && security["authentication"] == "server-certificate"
            && security["early_data"] == false
            && security["server_name"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
            && security["alpn"]
                == if protocol == "grpc" {
                    json!("h2")
                } else {
                    Value::Null
                }
    };
    if !encrypted || security["version"] != 1 || security["setup_timed"] != false {
        return Err("benchmark requires matching encrypted transport metadata; plaintext archives cannot qualify".into());
    }
    if iterations == 0
        || trial.protocol != protocol
        || trial.payload_bytes != bytes
        || trial.warmup != warmup
        || trial.iterations != iterations
        || trial.latency_ns.len() as u64 != iterations
        || trial.elapsed_ns == 0
        || trial.latency_ns.contains(&0)
        || trial
            .latency_ns
            .iter()
            .map(|&x| u128::from(x))
            .sum::<u128>()
            > u128::from(trial.elapsed_ns)
    {
        return Err("benchmark output does not match the requested workload/timing bounds".into());
    }
    Ok(())
}
pub fn percentile(samples: &mut [u64], percentile: usize) -> u64 {
    samples.sort_unstable();
    samples[(samples.len() * percentile).div_ceil(100).saturating_sub(1)]
}
pub fn matrix(trials: &[Trial]) -> Result<Vec<Value>> {
    let mut rows = vec![];
    let first = trials.first().ok_or("no benchmark trials")?;
    if trials.len() != PROTOCOLS.len() * PAYLOADS.len() * REPETITIONS
        || trials.iter().any(|t| {
            !PROTOCOLS.contains(&t.protocol.as_str()) || !PAYLOADS.contains(&t.payload_bytes)
        })
    {
        return Err(
            "benchmark must contain exactly the declared protocols, payloads and repetitions"
                .into(),
        );
    }
    if trials
        .iter()
        .any(|t| t.warmup != first.warmup || t.iterations != first.iterations)
    {
        return Err("benchmark cells have different sampling budgets".into());
    }
    for bytes in PAYLOADS {
        for protocol in PROTOCOLS {
            let cell: Vec<_> = trials
                .iter()
                .filter(|t| t.protocol == protocol && t.payload_bytes == bytes)
                .collect();
            if cell.len() != REPETITIONS {
                return Err(format!(
                    "missing/duplicate benchmark repetitions for {protocol}/{bytes}"
                )
                .into());
            }
            let first = cell[0];
            for trial in &cell {
                validate(trial, protocol, bytes, first.warmup, first.iterations)?;
                if trial.security != first.security {
                    return Err("mixed security contracts in benchmark cell".into());
                }
            }
            let mut samples: Vec<_> = cell
                .iter()
                .flat_map(|t| t.latency_ns.iter().copied())
                .collect();
            let mut medians: Vec<_> = cell
                .iter()
                .map(|t| percentile(&mut t.latency_ns.clone(), 50))
                .collect();
            let p50 = percentile(&mut samples, 50);
            let p95 = percentile(&mut samples, 95);
            let p99 = percentile(&mut samples, 99);
            medians.sort_unstable();
            let elapsed = cell.iter().map(|t| t.elapsed_ns).min().unwrap();
            let sample_trials: Vec<Vec<u64>> =
                cell.iter().map(|trial| trial.latency_ns.clone()).collect();
            let mut row = json!({"protocol":protocol,"security":first.security,"payload_bytes":bytes,"repetitions":cell.len(),"iterations_per_repetition":first.iterations,"p50_ns":p50,"p95_ns":p95,"p99_ns":p99,"median_range_ns":[medians[0],medians[REPETITIONS-1]],"throughput_estimator":"fastest_repetition","best_elapsed_ns":elapsed,"sequential_requests_per_second":first.iterations as f64 * 1e9 / elapsed as f64});
            row.as_object_mut().unwrap().extend(
                latency_summary::summarize(&sample_trials)
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            rows.push(row);
        }
    }
    Ok(rows)
}

/// Acceptance compares matched TLS capability RPC, at every payload size.
/// Keep raw distributions and tails in the report; the gate uses the minimum valid latency.
pub fn tcp_acceptance(rows: &[Value]) -> Result<()> {
    tcp_acceptance_for_driver(rows, "legacy")
}

pub fn tcp_acceptance_for_driver(rows: &[Value], driver: &str) -> Result<()> {
    let limit = match driver {
        "legacy" => 115,
        "portable" => 150,
        _ => return Err("missing or unknown measured TCP driver".into()),
    };
    for bytes in PAYLOADS {
        let latency = |protocol: &str| -> Result<u128> {
            let mut cells = rows
                .iter()
                .filter(|row| row["protocol"] == protocol && row["payload_bytes"] == bytes);
            let value = cells
                .next()
                .and_then(|row| row["min_ns"].as_u64())
                .filter(|&n| n > 0)
                .ok_or("missing TCP acceptance measurement")?;
            if cells.next().is_some() {
                return Err("duplicate TCP acceptance measurement".into());
            }
            Ok(u128::from(value))
        };
        let tcp = latency("tcp")?;
        let cpp = latency("capnp-cpp")?;
        if (driver == "legacy" || bytes <= 1024) && tcp * 100 > cpp * limit {
            return Err(format!(
                "{driver} TCP/C++ minimum exceeds {:.2}x at {bytes} bytes: {:.4}x",
                limit as f64 / 100.0,
                tcp as f64 / cpp as f64
            )
            .into());
        }
    }
    Ok(())
}
#[cfg(test)]
pub(crate) fn test_security(protocol: &str) -> Value {
    if protocol == "native" {
        json!({"version":1,"transport":"native","authentication":"pinned-mutual-identity","setup_timed":false})
    } else {
        json!({"version":1,"transport":"tls-tcp","tls":"1.3","authentication":"server-certificate","server_name":"localhost","early_data":false,"setup_timed":false,"alpn":if protocol == "grpc" { json!("h2") } else { Value::Null }})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tcp_target_requires_every_payload_and_accepts_exact_boundary() {
        let mut rows = Vec::new();
        for bytes in PAYLOADS {
            rows.push(json!({"protocol":"tcp","payload_bytes":bytes,"min_ns":115}));
            rows.push(json!({"protocol":"capnp-cpp","payload_bytes":bytes,"min_ns":100}));
        }
        assert!(tcp_acceptance(&rows).is_ok());
        rows[0]["min_ns"] = json!(116);
        assert!(tcp_acceptance(&rows).is_err());
        rows[0]["min_ns"] = json!(115);
        rows.push(rows[0].clone());
        assert!(tcp_acceptance(&rows).is_err());
        rows.pop();
        rows.pop();
        assert!(tcp_acceptance(&rows).is_err());
    }
    #[test]
    fn portable_target_limits_small_calls_and_retains_large_diagnostics() {
        let mut rows = Vec::new();
        for bytes in PAYLOADS {
            rows.push(json!({"protocol":"tcp","payload_bytes":bytes,"min_ns":if bytes <= 1024 { 150 } else { 900 }}));
            rows.push(json!({"protocol":"capnp-cpp","payload_bytes":bytes,"min_ns":100}));
        }
        assert!(tcp_acceptance_for_driver(&rows, "portable").is_ok());
        assert!(tcp_acceptance_for_driver(&rows, "legacy").is_err());
        assert!(tcp_acceptance_for_driver(&rows, "unknown").is_err());
        rows[0]["min_ns"] = json!(151);
        assert!(tcp_acceptance_for_driver(&rows, "portable").is_err());
        rows[0]["min_ns"] = json!(150);
        rows.pop();
        assert!(tcp_acceptance_for_driver(&rows, "portable").is_err());
    }
    #[test]
    fn legacy_or_mixed_measurement_contracts_cannot_qualify() {
        let mut trials = Vec::new();
        for bytes in PAYLOADS {
            for protocol in PROTOCOLS {
                for _ in 0..REPETITIONS {
                    trials.push(Trial {
                        measurement_version: MEASUREMENT_VERSION,
                        security: test_security(protocol),
                        protocol: protocol.into(),
                        payload_bytes: bytes,
                        warmup: 10,
                        iterations: 2,
                        elapsed_ns: 100,
                        latency_ns: vec![40, 40],
                    });
                }
            }
        }
        assert_eq!(matrix(&trials).unwrap().len(), 24);
        let summary = matrix(&trials).unwrap();
        assert_eq!(summary[0]["mean_ns"], 40.0);
        assert_eq!(summary[0]["repetition_mean_ns"], json!(vec![40.0; 5]));
        assert_eq!(summary[0]["repetition_mean_stddev_ns"], 0.0);
        let without_upstream: Vec<_> = trials
            .iter()
            .filter(|t| t.protocol != "capnp-rust")
            .cloned()
            .collect();
        assert!(matrix(&without_upstream).is_err());
        let mut legacy = serde_json::to_value(&trials[0]).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("measurement_version");
        assert!(serde_json::from_value::<Trial>(legacy).is_err());
        for version in [0, 1, MEASUREMENT_VERSION + 1] {
            trials[7].measurement_version = version;
            assert!(matrix(&trials).is_err());
        }
    }
    #[test]
    fn incomplete_or_impossible_benchmark_measurements_fail() {
        assert!(matrix(&[]).is_err());
        let mut t = Trial {
            measurement_version: MEASUREMENT_VERSION,
            security: test_security("native"),
            protocol: "native".into(),
            payload_bytes: 64,
            warmup: 10,
            iterations: 2,
            elapsed_ns: 100,
            latency_ns: vec![40, 40],
        };
        assert!(validate(&t, "native", 64, 10, 2).is_ok());
        t.latency_ns[1] = 70;
        assert!(validate(&t, "native", 64, 10, 2).is_err());
        t.latency_ns[1] = 0;
        assert!(validate(&t, "native", 64, 10, 2).is_err());
        assert_eq!(percentile(&mut [3, 1, 2, 4, 5], 95), 5);
    }
}
