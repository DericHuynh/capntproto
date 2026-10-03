//! Research probes, not production implementations or performance gates.
mod queue;
mod snapshot;
mod worker;

use serde_json::{json, Value};
use std::time::Duration;

fn metrics(operations: usize, elapsed: Duration, mut latency_ns: Vec<u64>) -> Value {
    latency_ns.sort_unstable();
    let percentile = |p: usize| {
        if latency_ns.is_empty() {
            0
        } else {
            latency_ns[(latency_ns.len() * p).div_ceil(100).saturating_sub(1)]
        }
    };
    json!({
        "operations": operations,
        "elapsed_ns": elapsed.as_nanos() as u64,
        "operations_per_second": operations as f64 / elapsed.as_secs_f64(),
        "ns_per_operation": elapsed.as_nanos() as f64 / operations as f64,
        "latency_samples": latency_ns.len(),
        "latency_p50_ns": percentile(50),
        "latency_p99_ns": percentile(99),
        "latency_max_ns": latency_ns.last().copied().unwrap_or(0),
        "validated": true,
    })
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        6,
        "usage: probe BASE KIND VARIANT THREADS BATCH"
    );
    let threads: usize = args[4].parse().unwrap();
    let batch: usize = args[5].parse().unwrap();
    assert!([1, 8].contains(&threads));
    let mut result = match args[2].as_str() {
        "queue" => queue::run(&args[3], threads, batch),
        "snapshot" => {
            assert_eq!(batch, 1);
            snapshot::run(&args[3], threads)
        }
        "worker" => {
            assert_eq!(batch, 1);
            worker::run(std::path::Path::new(&args[1]), &args[3], threads)
        }
        other => panic!("unknown kind {other}"),
    };
    result["kind"] = json!(args[2]);
    result["variant"] = json!(args[3]);
    result["threads"] = json!(threads);
    result["batch"] = json!(batch);
    println!("{result}");
}
