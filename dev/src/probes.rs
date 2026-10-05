//! Serial, source-bound research experiments. Per-run percentiles are never pooled.
use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use rand::{seq::SliceRandom, SeedableRng};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Subcommand)]
pub enum Args {
    Storage(Measurement),
    Resilience(Measurement),
    Concurrency(Measurement),
    Rpc {
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "target/debug/examples/rpc_pipeline_probe")]
        binary: PathBuf,
    },
    Summarize {
        #[arg(value_enum)]
        kind: Kind,
        directory: PathBuf,
    },
    CompareStorage {
        #[arg(long, default_value = "target/storage-benchmark/data")]
        base_dir: PathBuf,
        #[arg(long, default_value = "target/storage-benchmark/reports")]
        report_dir: PathBuf,
        #[arg(long,default_value_t=5,value_parser=clap::value_parser!(u32).range(1..))]
        trials: u32,
        #[arg(long,default_value_t=200,value_parser=clap::value_parser!(u32).range(20..))]
        iterations: u32,
    },
}
#[derive(Clone, Copy, ValueEnum)]
pub enum Kind {
    Storage,
    Resilience,
    Concurrency,
}
#[derive(Parser)]
pub struct Measurement {
    #[arg(long)]
    base: Option<PathBuf>,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    binary: Option<PathBuf>,
    #[arg(long,default_value_t=3,value_parser=clap::value_parser!(u32).range(1..))]
    trials: u32,
    #[arg(long)]
    summarize: Option<PathBuf>,
}
fn inputs() -> Result<BTreeMap<String, String>> {
    let root = crate::root();
    let mut result = BTreeMap::new();
    for name in [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "src",
        "dev",
        "crates",
        "test-support",
        "schemas",
        "examples",
        "benchmarks/concurrency",
        "benchmarks/storage",
        "vendor/provenance",
    ] {
        let path = root.join(name);
        if !path.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(path)
            .into_iter()
            .filter_entry(|e| !matches!(e.file_name().to_str(), Some("target" | ".git")))
        {
            let entry = entry?;
            if entry.file_type().is_file() {
                result.insert(
                    entry
                        .path()
                        .strip_prefix(&root)?
                        .to_string_lossy()
                        .into_owned(),
                    crate::file_hash(entry.path())?,
                );
            }
        }
    }
    Ok(result)
}
fn new_directory(path: &Path) -> Result<()> {
    ensure!(
        !path.exists(),
        "evidence directory already exists: {}",
        path.display()
    );
    std::fs::create_dir_all(path)?;
    Ok(())
}
fn environment(
    binary: &Path,
    base: Option<&Path>,
    sources: &BTreeMap<String, String>,
    notes: &str,
) -> Result<Value> {
    let mut env = json!({"started_utc":chrono::Utc::now().to_rfc3339(),"platform":format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH),"binary_sha256":crate::file_hash(binary)?,"sources_sha256":sources,"git_head":crate::process::text(Command::new("git").args(["rev-parse","HEAD"]))?,"rustc":crate::process::text(Command::new("rustc").arg("--version"))?,"notes":notes});
    if let Some(base) = base {
        env["filesystem"] = serde_json::from_slice(&crate::process::checked(
            Command::new("findmnt").args(["-J", "-T"]).arg(base),
            30,
        )?)?;
    }
    if let Ok(text) = std::fs::read_to_string("/proc/cpuinfo") {
        if let Some((_, model)) = text
            .lines()
            .find(|l| l.starts_with("model name"))
            .and_then(|l| l.split_once(':'))
        {
            env["cpu_model"] = json!(model.trim());
        }
    }
    if let Ok(governor) =
        std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
    {
        env["cpu0_governor"] = json!(governor.trim());
    }
    Ok(env)
}
fn cases(kind: Kind) -> Vec<Vec<String>> {
    let mut result = Vec::new();
    match kind {
        Kind::Storage => {
            for (mode, size, writes, held) in [
                ("whole", 2, 128, false),
                ("whole", 2, 128, true),
                ("components", 2, 128, false),
                ("components", 2, 128, true),
                ("components", 16, 128, false),
                ("components", 64, 128, false),
                ("components", 256, 128, false),
                ("direct", 2, 256, false),
                ("worker", 2, 256, false),
                ("batch", 1, 256, false),
                ("batch", 4, 256, false),
                ("batch", 16, 256, false),
            ] {
                result.push(vec![
                    mode.into(),
                    size.to_string(),
                    writes.to_string(),
                    held.to_string(),
                ]);
            }
        }
        Kind::Resilience => {
            for case in [
                "baseline",
                "steady-stall",
                "overload-8",
                "overload-64",
                "overload-256",
                "deadline-25ms",
                "mixed-4mib",
                "mixed-128kib",
                "lost-reply",
            ] {
                result.push(vec![case.into()]);
            }
        }
        Kind::Concurrency => {
            for (kind, variants, batches) in [
                (
                    "queue",
                    &["std", "parking", "array", "tokio"][..],
                    &[1, 16][..],
                ),
                (
                    "snapshot",
                    &["mutex", "rwlock", "guard", "owned"][..],
                    &[1][..],
                ),
                ("worker", &["status", "write"][..], &[1][..]),
            ] {
                for variant in variants {
                    for threads in [1, 8] {
                        for batch in batches {
                            result.push(vec![
                                kind.into(),
                                (*variant).into(),
                                threads.to_string(),
                                batch.to_string(),
                            ]);
                        }
                    }
                }
            }
        }
    }
    result
}
#[cfg(unix)]
fn child_cpu() -> Result<f64> {
    use nix::sys::{
        resource::{getrusage, UsageWho},
        time::TimeValLike,
    };
    let usage = getrusage(UsageWho::RUSAGE_CHILDREN)?;
    Ok(
        (usage.user_time().num_microseconds() + usage.system_time().num_microseconds()) as f64
            / 1e6,
    )
}
#[cfg(not(unix))]
fn child_cpu() -> Result<f64> {
    Ok(0.0)
}
fn measure(kind: Kind, args: Measurement) -> Result<()> {
    if let Some(directory) = args.summarize {
        return summarize(kind, &directory);
    }
    let base = args
        .base
        .context("measurement requires --base")?
        .canonicalize()?;
    ensure!(base.is_dir(), "--base must be a directory");
    let output = args.output.context("measurement requires --output")?;
    let binary = args
        .binary
        .unwrap_or_else(|| {
            PathBuf::from(match kind {
                Kind::Storage => "target/release/examples/storage_probe",
                Kind::Resilience => "target/release/examples/storage_resilience",
                Kind::Concurrency => {
                    "target/concurrency-research/release/capntproto-concurrency-probe"
                }
            })
        })
        .canonicalize()?;
    new_directory(&output)?;
    let sources = inputs()?;
    let seed = if matches!(kind, Kind::Storage) {
        20261001
    } else {
        20261002
    };
    let notes=match kind{Kind::Storage=>"Uncontrolled shared host; warm-cache reads/reopen; no timing gates; logical file bytes, not device writes.",Kind::Resilience=>"Uncontrolled shared host. One-second fixed-rate arrival window, no client retries. Real fsync per commit; one injected worker sleep, not disk EIO. Byte credits account for queued/executing payloads, not RSS or history/index bytes.",Kind::Concurrency=>"Uncontrolled shared host. Queue probes yield on full/empty; saturation, not production parking or admission. Snapshot writers sleep 100us between publications. Worker cases use one outstanding request per producer, default budgets, real fsync for writes. No disk errors, CPU pinning, warmup exclusion or performance gates."};
    let mut env = environment(&binary, Some(&base), &sources, notes)?;
    env["trials"] = json!(args.trials);
    env["order_seed"] = json!(seed);
    env["order_generator"] =
        json!("rand 0.9 StdRng (ChaCha12); explicit commands retained in runs.jsonl");
    crate::write_json(output.join("environment.json"), &env)?;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut file = std::fs::File::create(output.join("runs.jsonl"))?;
    for trial in 1..=args.trials {
        let mut order = cases(kind);
        order.shuffle(&mut rng);
        for case in order {
            let mut command = vec![binary.display().to_string(), base.display().to_string()];
            command.extend(case.clone());
            let before = child_cpu()?;
            let bytes = crate::process::checked(Command::new(&binary).args(&command[1..]), 180)?;
            let after = child_cpu()?;
            let mut row: Value = serde_json::from_slice(&bytes)?;
            row["trial"] = json!(trial);
            row["command"] = json!(command);
            if matches!(kind, Kind::Concurrency) && cfg!(unix) {
                row["process_cpu_seconds"] = json!(after - before);
            }
            writeln!(file, "{row}")?;
            file.flush()?;
            println!("trial {trial}: {}", case.join("/"));
        }
    }
    ensure!(sources == inputs()?, "probe inputs changed during run");
    summarize(kind, &output)
}
fn field(row: &Value, path: &str) -> Result<f64> {
    let mut value = row;
    for key in path.split('.') {
        value = &value[key];
    }
    let value = value
        .as_f64()
        .with_context(|| format!("missing/non-numeric metric {path}"))?;
    ensure!(value.is_finite(), "invalid metric {path}");
    Ok(value)
}
pub fn distribution(mut values: Vec<f64>) -> Result<Value> {
    ensure!(
        !values.is_empty() && values.iter().all(|v| v.is_finite()),
        "empty/non-finite distribution"
    );
    values.sort_by(f64::total_cmp);
    let n = values.len();
    let median = if n % 2 == 1 {
        values[n / 2]
    } else {
        values[n / 2 - 1] / 2.0 + values[n / 2] / 2.0
    };
    Ok(json!({"median":median,"min":values[0],"max":values[n-1]}))
}
fn metrics(rows: &[Value], fields: &[&str]) -> Result<Value> {
    let mut result = json!({});
    for name in fields {
        result[*name] = distribution(rows.iter().map(|r| field(r, name)).collect::<Result<_>>()?)?;
    }
    Ok(result)
}
pub fn summarize(kind: Kind, directory: &Path) -> Result<()> {
    let mut groups: BTreeMap<String, (Value, Vec<Value>)> = BTreeMap::new();
    for line in std::fs::read_to_string(directory.join("runs.jsonl"))?.lines() {
        let row: Value = serde_json::from_str(line)?;
        let identity = match kind {
            Kind::Storage => {
                json!({"mode":row["mode"],"size":row.get("components").or_else(||row.get("batch_size")).cloned().unwrap_or(json!(0)),"held_snapshots":row.get("held_snapshots").cloned().unwrap_or(json!(false))})
            }
            Kind::Resilience => {
                ensure!(row["verified_reopen"] == true, "reopen verification failed");
                if row["scenario"] != "lost-reply" {
                    let n = |key| crate::number(&row, key);
                    ensure!(
                        n("offered")?
                            == n("admitted")?
                                .checked_add(n("rejected_count")?)
                                .and_then(|x| x.checked_add(n("rejected_bytes").ok()?))
                                .context("counter overflow")?,
                        "offered requests do not balance"
                    );
                    ensure!(
                        n("admitted")?
                            == n("committed")?
                                .checked_add(n("expired_before_execution")?)
                                .context("counter overflow")?,
                        "admitted requests do not balance"
                    );
                    ensure!(
                        n("peak_outstanding_count")? <= n("count_limit")?
                            && n("peak_outstanding_payload_bytes")? <= n("payload_byte_limit")?,
                        "worker budget exceeded"
                    );
                }
                json!({"scenario":row["scenario"]})
            }
            Kind::Concurrency => {
                ensure!(
                    row["validated"] == true && crate::number(&row, "operations")? > 0,
                    "invalid concurrency evidence"
                );
                if row["kind"] == "worker" {
                    ensure!(
                        row["verified_reopen"] == true && row["rejected"] == 0,
                        "invalid worker evidence"
                    );
                }
                json!({"kind":row["kind"],"variant":row["variant"],"threads":row["threads"],"batch":row["batch"]})
            }
        };
        groups
            .entry(identity.to_string())
            .or_insert_with(|| (identity, Vec::new()))
            .1
            .push(row);
    }
    ensure!(!groups.is_empty(), "no probe runs");
    let mut scenarios = Vec::new();
    for (_, (mut item, rows)) in groups {
        let fields: Vec<&str> = match kind {
            Kind::Storage => match item["mode"].as_str().context("missing mode")? {
                "whole" | "components" => vec![
                    "bytes_per_update",
                    "warm_acquire_and_hot_read_mean_us",
                    "warm_reopen_us",
                    "history_checkpoint_bytes",
                    "history_compact_us",
                    "commit_including_encode.p50_us",
                    "commit_including_encode.p99_us",
                    "mapped_before.mappings",
                    "mapped_before.virtual_bytes",
                    "mapped_after_compact.mappings",
                    "mapped_after_release.mappings",
                ],
                "batch" => vec![
                    "mean_us_per_object_update",
                    "batch_commit.p50_us",
                    "batch_commit.p99_us",
                    "syncs",
                ],
                _ => vec![
                    "raw_component_commit.p50_us",
                    "raw_component_commit.p99_us",
                    "timer_lateness.p99_us",
                    "timer_lateness.max_us",
                    "timer_lateness.count",
                    "elapsed_us",
                ],
            },
            Kind::Resilience => {
                if item["scenario"] == "lost-reply" {
                    vec!["committed"]
                } else {
                    vec![
                        "committed",
                        "admitted",
                        "rejected_count",
                        "rejected_bytes",
                        "rejected_large",
                        "committed_large",
                        "expired_before_execution",
                        "committed_after_deadline",
                        "peak_outstanding_count",
                        "peak_outstanding_payload_bytes",
                        "pause_us",
                        "drain_after_arrival_window_us",
                        "total_elapsed_us",
                        "generator_lateness.p99_us",
                        "generator_lateness.max_us",
                        "success_latency.p50_us",
                        "success_latency.p99_us",
                        "queue_age_at_dequeue.p99_us",
                        "commit_service.p50_us",
                        "commit_service.p99_us",
                    ]
                }
            }
            Kind::Concurrency => [
                "operations_per_second",
                "ns_per_operation",
                "elapsed_ns",
                "latency_p50_ns",
                "latency_p99_ns",
                "latency_max_ns",
                "full_retries",
                "empty_polls",
                "publications",
                "process_cpu_seconds",
            ]
            .into_iter()
            .filter(|name| rows[0].get(*name).is_some())
            .collect(),
        };
        item["trials"] = json!(rows.len());
        item["metrics"] = metrics(&rows, &fields)?;
        scenarios.push(item);
    }
    crate::write_json(
        directory.join("summary.json"),
        &json!({"aggregation":"Median and range of per-run statistics, never pooled percentiles. Rejections and expiries are separate outcomes, not successful latencies. Snapshot cases have no latency samples (zero fields mean not measured). Process CPU includes setup and verification; elapsed_ns is the timed region.","scenarios":scenarios}),
    )
}
fn rpc(output: &Path, binary: &Path) -> Result<()> {
    let binary = binary.canonicalize()?;
    new_directory(output)?;
    let sources = inputs()?;
    let mut outputs = Vec::new();
    for _ in 0..3 {
        let result: Value =
            serde_json::from_slice(&crate::process::checked(&mut Command::new(&binary), 30)?)?;
        let scenarios = result["scenarios"]
            .as_array()
            .context("missing RPC scenarios")?;
        ensure!(
            scenarios.len() == 15 && scenarios.iter().all(|r| r["validated"] == true),
            "RPC scenarios did not validate"
        );
        outputs.push(result);
    }
    ensure!(
        outputs.windows(2).all(|w| w[0] == w[1]),
        "Virtual-clock results differed across repetitions"
    );
    ensure!(sources == inputs()?, "probe inputs changed during run");
    let mut env=environment(&binary,None,&sources,"Finite protocol scenarios, not wall-clock benchmarks. Tokio paused time advances only when execution has no ready work. Per-frame delays overlap, preserve order and model propagation without bandwidth, loss, jitter, TLS or QUIC. Zero-delay virtual durations do not measure CPU cost.")?;
    env["identical_repetitions"] = json!(3);
    crate::write_json(output.join("environment.json"), &env)?;
    crate::write_json(output.join("results.json"), &outputs[0])?;
    println!("15 scenarios validated; three fresh processes produced identical virtual times and wire counters.");
    Ok(())
}
const EAE_SHA: &str = "55872653058ee56813a3417df242760b81dc002e53500ca13c7c696335cf7f5b";
const COMPARISONS: [(&str, u32, u32, u32, u32, u32, u32); 8] = [
    ("small_replace", 32, 64, 64, 100, 10, 1),
    ("4k_replace", 32, 4096, 4096, 100, 10, 1),
    ("64k_replace", 8, 65536, 65536, 100, 10, 1),
    ("1m_replace", 2, 1048576, 1048576, 20, 10, 1),
    ("4k_field", 32, 4096, 8, 100, 10, 1),
    ("64k_field", 8, 65536, 8, 100, 10, 1),
    ("sparse_64k_field", 8, 65536, 8, 100, 10, 16),
    ("snapshot_64k_field", 8, 65536, 8, 100, 1, 1),
];
fn prepare_eae() -> Result<()> {
    let archive = crate::root().join("research/EAE-Reconstruction.zip");
    ensure!(
        crate::file_hash(&archive)? == EAE_SHA,
        "EAE archive hash mismatch"
    );
    let output = crate::root().join("target/eae-benchmark");
    let mut source = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
    for i in 0..source.len() {
        let entry = source.by_index(i)?;
        let path = entry
            .enclosed_name()
            .context("archive path escapes destination")?;
        ensure!(
            entry.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
            "archive contains symlink"
        );
        for ancestor in output.join(path).ancestors() {
            ensure!(
                !ancestor.is_symlink(),
                "archive destination contains symlink"
            );
        }
    }
    source.extract(output)?;
    Ok(())
}
fn compare(base: &Path, output: &Path, trials: u32, iterations: u32) -> Result<()> {
    std::fs::create_dir_all(base)?;
    new_directory(output)?;
    prepare_eae()?;
    let base = base.canonicalize()?;
    let target = crate::root().join("target/storage-benchmark");
    let manifest = crate::root().join("benchmarks/storage/Cargo.toml");
    if !manifest.with_file_name("Cargo.lock").exists() {
        crate::process::live(
            Command::new("cargo")
                .args([
                    "+1.97.0",
                    "generate-lockfile",
                    "--offline",
                    "--manifest-path",
                ])
                .arg(&manifest)
                .env("CARGO_TARGET_DIR", &target),
            180,
            &Default::default(),
        )?;
    }
    crate::process::live(
        Command::new("cargo")
            .args([
                "+1.97.0",
                "auditable",
                "build",
                "--release",
                "--locked",
                "--manifest-path",
            ])
            .arg(&manifest)
            .env("CARGO_TARGET_DIR", &target),
        1800,
        &Default::default(),
    )?;
    let binary = target.join("release/capntproto-storage-comparison");
    let sources = inputs()?;
    let mut rows = Vec::new();
    let mut file = std::fs::File::create(output.join("runs.jsonl"))?;
    for trial in 0..trials {
        for (name, objects, entry, edit, every, snapshots, factor) in COMPARISONS {
            for backend in if trial % 2 == 0 {
                ["store", "eae"]
            } else {
                ["eae", "store"]
            } {
                let mut command = vec![
                    binary.display().to_string(),
                    backend.into(),
                    base.display().to_string(),
                ];
                command.extend(
                    [objects, entry, edit, iterations, every, snapshots, factor]
                        .map(|n| n.to_string()),
                );
                let mut row: Value = serde_json::from_slice(&crate::process::checked(
                    Command::new(&binary).args(&command[1..]),
                    120,
                )?)?;
                row["case"] = json!(name);
                row["trial"] = json!(trial + 1);
                row["command"] = json!(command);
                writeln!(file, "{row}")?;
                file.flush()?;
                println!(
                    "{}/{trials} {name} {backend}: {:.1} us median commit",
                    trial + 1,
                    field(&row, "commits.p50_ns")? / 1000.0
                );
                rows.push(row);
            }
        }
    }
    let mut summary = json!({});
    for (name, ..) in COMPARISONS {
        summary[name] = json!({});
        for backend in ["store", "eae"] {
            let group: Vec<_> = rows
                .iter()
                .filter(|r| r["case"] == name && r["backend"] == backend)
                .collect();
            let mut values = json!({});
            for (label, path, scale) in [
                ("commit_p50_us", "commits.p50_ns", 1000.0),
                ("commit_p99_us", "commits.p99_ns", 1000.0),
                ("workload_ms", "workload_wall_ns", 1e6),
                ("first_snapshot_us", "first_snapshots.p50_ns", 1000.0),
                ("repeated_snapshot_us", "repeated_snapshots.p50_ns", 1000.0),
                ("reopen_with_log_ms", "reopen_with_log_ns", 1e6),
                ("reopen_checkpoint_ms", "reopen_checkpoint_ns", 1e6),
                ("total_written_bytes", "total_written_bytes", 1.0),
                ("process_peak_rss_kib", "process_peak_rss_kib", 1.0),
            ] {
                let stats = distribution(
                    group
                        .iter()
                        .map(|r| Ok(field(r, path)? / scale))
                        .collect::<Result<_>>()?,
                )?;
                values[label] = stats["median"].clone();
                if label == "workload_ms" {
                    values["workload_ms_range"] = json!([stats["min"], stats["max"]]);
                }
            }
            values["write_amplification"] = distribution(
                group
                    .iter()
                    .map(|r| {
                        Ok(field(r, "total_written_bytes")? / field(r, "logical_changed_bytes")?)
                    })
                    .collect::<Result<_>>()?,
            )?["median"]
                .clone();
            summary[name][backend] = values;
        }
    }
    ensure!(sources == inputs()?, "benchmark inputs changed during run");
    let mut report=environment(&binary,Some(&base),&sources,"Comparison adapter, not full ORM/history/capability integration. Fixed slots omit allocator/graph relocation; timings from one shared host; RSS is process peak; coexistence bytes are logical, not physical peak; warm-cache reopen; fsync calls do not prove hardware power-loss behavior.")?;
    report["summary"] = summary;
    report["trials"] = json!(trials);
    report["iterations"] = json!(iterations);
    report["runs"] = json!(rows.len());
    report["eae_archive_sha256"] = json!(EAE_SHA);
    report["scope"]=json!("single writer; fixed-size latest-published objects; per-object CAS metadata; durable single-object transactions; one held snapshot and 32 repeated same-generation snapshots; all scheduled plus final checkpoints accounted");
    crate::write_json(output.join("comparison.json"), &report)
}
pub fn run(args: Args) -> Result<()> {
    match args {
        Args::Storage(a) => measure(Kind::Storage, a),
        Args::Resilience(a) => measure(Kind::Resilience, a),
        Args::Concurrency(a) => measure(Kind::Concurrency, a),
        Args::Rpc { output, binary } => rpc(&output, &binary),
        Args::Summarize { kind, directory } => summarize(kind, &directory),
        Args::CompareStorage {
            base_dir,
            report_dir,
            trials,
            iterations,
        } => compare(&base_dir, &report_dir, trials, iterations),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scenarios_preserve_experiment_scope() {
        assert_eq!(cases(Kind::Storage).len(), 12);
        assert_eq!(cases(Kind::Resilience).len(), 9);
        assert_eq!(cases(Kind::Concurrency).len(), 28);
    }
    #[test]
    fn medians_do_not_pool_samples() {
        assert_eq!(
            distribution(vec![10.0, 100.0, 30.0]).unwrap()["median"],
            30.0
        );
        assert_eq!(distribution(vec![10.0, 100.0]).unwrap()["median"], 55.0);
        assert!(distribution(vec![f64::NAN]).is_err());
    }
    #[test]
    fn failed_reopen_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        crate::write(
            dir.path().join("runs.jsonl"),
            "{\"scenario\":\"lost-reply\",\"verified_reopen\":false,\"committed\":1}\n",
        )
        .unwrap();
        assert!(summarize(Kind::Resilience, dir.path()).is_err());
    }

    #[test]
    fn summaries_preserve_frozen_experiment_statistics() {
        // Preserve even the last binary floating-point bit when reading measurements.
        // JSON permits integer and floating-point representations of the same count.
        fn numbers(value: &mut Value) {
            match value {
                Value::Number(n) => *value = json!(n.as_f64().unwrap()),
                Value::Array(items) => items.iter_mut().for_each(numbers),
                Value::Object(items) => items.values_mut().for_each(numbers),
                _ => (),
            }
        }
        fn scenarios(mut report: Value) -> Vec<Value> {
            numbers(&mut report);
            let mut scenarios = report["scenarios"].as_array().unwrap().clone();
            scenarios.sort_by_key(Value::to_string);
            scenarios
        }
        for (kind, folder) in [
            (Kind::Storage, "storage-next"),
            (Kind::Resilience, "storage-resilience"),
            (Kind::Concurrency, "concurrency"),
        ] {
            let source = crate::root().join(format!("research/reports/{folder}/2026-10-01"));
            let dir = tempfile::tempdir().unwrap();
            std::fs::copy(source.join("runs.jsonl"), dir.path().join("runs.jsonl")).unwrap();
            summarize(kind, dir.path()).unwrap();
            assert_eq!(
                scenarios(crate::read_json(dir.path().join("summary.json")).unwrap()),
                scenarios(crate::read_json(source.join("summary.json")).unwrap()),
                "{folder} statistics changed"
            );
        }
    }
}
