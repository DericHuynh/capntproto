//! The complete hosted runtime matrix, executed sequentially on one pinned CPU.
//! Smoke output is deliberately ineligible for performance publication.
use crate::process::Cancellation;
use anyhow::{ensure, Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
const RUNTIMES: [&str; 4] = ["tokio", "tokio-driven", "monoio-epoll", "monoio-uring"];
const TRANSPORTS: [&str; 2] = ["tls", "webtransport"];
const DRIVERS: [&str; 2] = ["legacy", "portable"];
const METRICS: [&str; 9] = [
    "wall_ns",
    "cpu_ns",
    "cycles",
    "blocks",
    "bytes",
    "reads",
    "writes",
    "received_bytes",
    "transmitted_bytes",
];
#[derive(Parser)]
pub struct Args {
    #[arg(long)]
    pub worker: PathBuf,
    #[arg(long)]
    pub output: PathBuf,
    #[arg(long)]
    pub certificates: PathBuf,
    /// Required for native measurements; binds worker/controller to the audited bundle.
    #[arg(long)]
    pub manifest: Option<PathBuf>,
    /// Run every fixture once for correctness; permit an explicitly unavailable io_uring.
    #[arg(long)]
    pub smoke: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub runtime: String,
    pub transport: String,
    pub driver: String,
    pub case: String,
    pub iterations: u64,
    pub warmup: u64,
}
impl Cell {
    fn io_level(&self) -> &str {
        if self.transport == "tls" {
            "ciphertext-completions"
        } else {
            "rpc-stream-completions"
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trial {
    pub cell: Cell,
    pub repetition: usize,
    pub output: Value,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stats {
    pub min: f64,
    pub mean: f64,
    pub sd: f64,
    pub max: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub cell: Cell,
    pub metrics: BTreeMap<String, Option<Stats>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub version: u64,
    pub mode: String,
    pub status: String,
    pub source_id: Option<String>,
    pub certificates: BTreeMap<String, String>,
    pub worker_sha256: String,
    pub runner_sha256: String,
    pub trials_sha256: Option<String>,
    pub repetitions: usize,
    pub rows: Vec<Row>,
    pub unsupported: BTreeMap<String, String>,
    pub error: Option<String>,
}
pub fn plan(smoke: bool) -> Vec<Cell> {
    let mut cells = Vec::new();
    for runtime in RUNTIMES {
        for transport in TRANSPORTS {
            for driver in DRIVERS {
                for (case, iterations, warmup) in [
                    ("unary", 1024, 32),
                    ("large", 32, 4),
                    ("dependent", 128, 8),
                    ("pipelined", 128, 8),
                    ("concurrent-32", 256, 16),
                    ("deep-32-concurrent-32", 4, 2),
                    ("callback", 128, 8),
                ] {
                    cells.push(Cell {
                        runtime: runtime.into(),
                        transport: transport.into(),
                        driver: driver.into(),
                        case: case.into(),
                        iterations: if smoke { 1 } else { iterations },
                        warmup: if smoke { 1 } else { warmup },
                    });
                }
            }
        }
    }
    cells
}
fn stats(values: &[f64]) -> Stats {
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = if values.len() > 1 {
        values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (values.len() - 1) as f64
    } else {
        0.
    };
    Stats {
        min: values.iter().copied().fold(f64::INFINITY, f64::min),
        mean,
        sd: variance.sqrt(),
        max: values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    }
}
fn validate_trial(trial: &Trial, repetitions: usize) -> Result<()> {
    let output = &trial.output;
    let cell = &trial.cell;
    ensure!(
        trial.repetition < repetitions && output["version"] == 1,
        "invalid repetition or worker version"
    );
    for (key, expected) in [
        ("runtime", &cell.runtime),
        ("transport", &cell.transport),
        ("driver", &cell.driver),
        ("case", &cell.case),
    ] {
        ensure!(
            output[key] == expected.as_str(),
            "worker identity mismatch: {key}"
        );
    }
    ensure!(
        output["iterations"] == cell.iterations
            && output["warmup"] == cell.warmup
            && output["io_level"] == cell.io_level(),
        "worker budget or IO level mismatch"
    );
    let sample = &output["sample"];
    for metric in METRICS {
        if metric == "cycles" && sample[metric].is_null() {
            ensure!(
                sample["cycles_error"]
                    .as_str()
                    .is_some_and(|v| !v.is_empty()),
                "missing cycle unavailability reason"
            );
            continue;
        }
        if metric == "cycles" {
            ensure!(
                sample["cycles_error"].is_null(),
                "available cycles have an error"
            );
        }
        let value = sample[metric]
            .as_u64()
            .context(format!("invalid metric {metric}"))?;
        if ["wall_ns", "cpu_ns", "cycles"].contains(&metric) {
            ensure!(value > 0, "zero {metric}");
        }
    }
    ensure!(
        sample["callgrind_markers"].is_boolean(),
        "missing instrumentation metadata"
    );
    Ok(())
}
pub fn aggregate(
    trials: &[Trial],
    smoke: bool,
    repetitions: usize,
    unsupported: &BTreeMap<String, String>,
) -> Result<Vec<Row>> {
    ensure!(
        repetitions == if smoke { 1 } else { 5 },
        "native measurements require five repetitions"
    );
    ensure!(
        unsupported.is_empty() || smoke && unsupported.keys().all(|r| r == "monoio-uring"),
        "native runtime matrix is incomplete"
    );
    let expected: BTreeSet<_> = plan(smoke)
        .into_iter()
        .filter(|c| !unsupported.contains_key(&c.runtime))
        .collect();
    let mut cells: BTreeMap<Cell, BTreeMap<usize, &Trial>> = BTreeMap::new();
    for trial in trials {
        ensure!(
            expected.contains(&trial.cell),
            "unexpected runtime matrix cell"
        );
        validate_trial(trial, repetitions)?;
        ensure!(
            cells
                .entry(trial.cell.clone())
                .or_default()
                .insert(trial.repetition, trial)
                .is_none(),
            "duplicate repetition"
        );
    }
    ensure!(
        cells.keys().cloned().collect::<BTreeSet<_>>() == expected,
        "missing runtime matrix cell"
    );
    let mut rows = Vec::new();
    for (cell, samples) in cells {
        ensure!(samples.len() == repetitions, "missing runtime repetitions");
        let mut metrics = BTreeMap::new();
        for metric in METRICS {
            let values = samples
                .values()
                .map(|s| {
                    s.output["sample"][metric]
                        .as_u64()
                        .map(|v| v as f64 / cell.iterations as f64)
                })
                .collect::<Option<Vec<_>>>();
            metrics.insert(metric.into(), values.as_deref().map(stats));
        }
        rows.push(Row { cell, metrics });
    }
    Ok(rows)
}
fn pin_cpu() -> Result<Value> {
    #[cfg(target_os = "linux")]
    {
        use nix::{
            sched::{sched_getaffinity, sched_setaffinity, CpuSet},
            unistd::Pid,
        };
        let allowed = sched_getaffinity(Pid::from_raw(0))?;
        let cpu = (0..CpuSet::count())
            .find(|cpu| allowed.is_set(*cpu).unwrap_or(false))
            .context("no allowed benchmark CPU")?;
        let mut pinned = CpuSet::new();
        pinned.set(cpu)?;
        sched_setaffinity(Pid::from_raw(0), &pinned)?;
        Ok(
            json!({"cpu":cpu,"cpu_affinity":fs::read_to_string("/proc/self/status")?.lines().find(|l|l.starts_with("Cpus_allowed_list:")).unwrap_or("")}),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        anyhow::bail!("runtime benchmark campaign requires Linux CPU affinity")
    }
}
fn invoke(worker: &Path, arguments: &[String], cancel: &Cancellation) -> Result<Value> {
    let output = crate::process::run(Command::new(worker).args(arguments), 150, None, cancel)?;
    ensure!(
        output.code == 0,
        "runtime worker failed ({}): {}",
        output.code,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).context("runtime worker must emit one JSON result")
}
fn identity(manifest: &Value, worker_hash: &str, runner_hash: &str) -> Result<String> {
    let source = crate::string(manifest, "source_id")?;
    ensure!(
        crate::reports::data::matches(r"^[0-9a-f]{64}$", source),
        "invalid source identity"
    );
    ensure!(
        manifest["binaries"]["runtime-bench"] == worker_hash
            && manifest["binaries"]["runtime-runner"] == runner_hash,
        "runtime binaries differ from audited bundle"
    );
    Ok(source.into())
}
pub fn run(args: Args) -> Result<()> {
    ensure!(
        args.smoke || args.manifest.is_some(),
        "native campaign requires --manifest"
    );
    ensure!(
        !args.output.exists(),
        "output already exists; preserve previous evidence"
    );
    let worker = args.worker.canonicalize()?;
    let runner = std::env::current_exe()?;
    let mut summary = Summary {
        version: 1,
        mode: if args.smoke { "smoke" } else { "native" }.into(),
        status: "running".into(),
        source_id: None,
        certificates: ["ca.der", "server.der"]
            .into_iter()
            .filter_map(|name| {
                let path = args.certificates.join(name);
                path.is_file()
                    .then(|| crate::file_hash(path).map(|hash| (name.to_string(), hash)))
            })
            .collect::<Result<_>>()?,
        worker_sha256: crate::file_hash(&worker)?,
        runner_sha256: crate::file_hash(runner)?,
        trials_sha256: None,
        repetitions: if args.smoke { 1 } else { 5 },
        rows: Vec::new(),
        unsupported: BTreeMap::new(),
        error: None,
    };
    if let Some(path) = &args.manifest {
        summary.source_id = Some(identity(
            &crate::read_json(path)?,
            &summary.worker_sha256,
            &summary.runner_sha256,
        )?);
        ensure!(
            json!(summary.certificates)
                == crate::read_json(path)?["security_fixture"]["certificate_sha256"],
            "runtime certificates differ from audited bundle"
        );
    }
    fs::create_dir_all(&args.output)?;
    crate::write_json(args.output.join("summary.json"), &summary)?;
    let cancel = Cancellation::install()?;
    let result = (|| -> Result<()> {
        let placement = pin_cpu()?;
        let environment = json!({"placement":placement,"kernel":crate::process::text(Command::new("uname").arg("-a"))?,"cpu":fs::read_to_string("/proc/cpuinfo")?,"io_uring_disabled":fs::read_to_string("/proc/sys/kernel/io_uring_disabled").ok(),"topology":"both loopback peers in one process on one pinned runtime thread","clock":"process CPU ns; user-space thread cycles when perf_event_open is available","profile":"release in CI; smoke is not performance evidence"});
        crate::write_json(args.output.join("environment.json"), &environment)?;
        for runtime in RUNTIMES {
            let probe = invoke(
                &worker,
                &["--probe-runtime".into(), runtime.into()],
                &cancel,
            )?;
            ensure!(
                probe["runtime"] == runtime && probe["available"].is_boolean(),
                "invalid runtime capability probe"
            );
            if probe["available"] == false {
                let reason = crate::string(&probe, "reason")?.to_owned();
                ensure!(
                    args.smoke && runtime == "monoio-uring",
                    "required runtime {runtime} unavailable: {reason}"
                );
                summary.unsupported.insert(runtime.into(), reason);
            }
        }
        let cells = plan(args.smoke)
            .into_iter()
            .filter(|c| !summary.unsupported.contains_key(&c.runtime))
            .collect::<Vec<_>>();
        let mut raw = fs::File::create(args.output.join("trials.jsonl"))?;
        let mut trials = Vec::new();
        for repetition in 0..summary.repetitions {
            // Rotate configuration order between repetitions to avoid a fixed warm/cold winner.
            for step in 0..cells.len() {
                let cell = &cells[(step + repetition * 17) % cells.len()];
                println!(
                    "Runtime trial {}/{}: {} / {} / {} / {}",
                    repetition + 1,
                    summary.repetitions,
                    cell.runtime,
                    cell.transport,
                    cell.driver,
                    cell.case
                );
                let arguments = [
                    ("--runtime", cell.runtime.clone()),
                    ("--transport", cell.transport.clone()),
                    ("--driver", cell.driver.clone()),
                    ("--case", cell.case.clone()),
                    ("--iterations", cell.iterations.to_string()),
                    ("--warmup", cell.warmup.to_string()),
                    (
                        "--certificates",
                        args.certificates.canonicalize()?.display().to_string(),
                    ),
                ]
                .into_iter()
                .flat_map(|(key, value)| [key.to_string(), value])
                .collect::<Vec<_>>();
                let output = invoke(&worker, &arguments, &cancel)?;
                let trial = Trial {
                    cell: cell.clone(),
                    repetition,
                    output,
                };
                validate_trial(&trial, summary.repetitions)?;
                serde_json::to_writer(&mut raw, &trial)?;
                writeln!(raw)?;
                raw.flush()?;
                trials.push(trial);
            }
        }
        summary.rows = aggregate(
            &trials,
            args.smoke,
            summary.repetitions,
            &summary.unsupported,
        )?;
        summary.trials_sha256 = Some(crate::file_hash(args.output.join("trials.jsonl"))?);
        summary.status = "passed".into();
        Ok(())
    })();
    if let Err(error) = &result {
        summary.status = "failed".into();
        summary.error = Some(format!("{error:#}"));
    }
    crate::write_json(args.output.join("summary.json"), &summary)?;
    crate::write(args.output.join("README.md"), markdown(&summary))?;
    result
}
/// Recompute every published counter from source-bound raw samples, not supplied aggregates.
pub fn verified(directory: &Path, manifest: &Value) -> Result<Summary> {
    let summary: Summary = serde_json::from_slice(&fs::read(directory.join("summary.json"))?)?;
    ensure!(
        summary.version == 1
            && summary.mode == "native"
            && summary.status == "passed"
            && summary.error.is_none(),
        "runtime campaign is not complete native evidence"
    );
    ensure!(
        Some(identity(
            manifest,
            &summary.worker_sha256,
            &summary.runner_sha256
        )?) == summary.source_id,
        "runtime source differs from build"
    );
    ensure!(
        json!(summary.certificates) == manifest["security_fixture"]["certificate_sha256"],
        "runtime certificate identity mismatch"
    );
    ensure!(
        summary.trials_sha256.as_deref()
            == Some(crate::file_hash(directory.join("trials.jsonl"))?.as_str()),
        "runtime trial hash mismatch"
    );
    let trials = fs::read_to_string(directory.join("trials.jsonl"))?
        .lines()
        .map(serde_json::from_str::<Trial>)
        .collect::<serde_json::Result<Vec<_>>>()?;
    let expected = aggregate(&trials, false, summary.repetitions, &summary.unsupported)?;
    ensure!(
        summary.rows == expected,
        "runtime summaries differ from raw samples"
    );
    Ok(summary)
}
pub const COLUMNS: [&str; 15] = [
    "Runtime",
    "Transport",
    "RPC driver",
    "Case (operation or batch)",
    "Runs",
    "Wall ns: min / mean ± SD",
    "CPU ns: min / mean ± SD",
    "User cycles: min / mean ± SD",
    "Allocations: min / mean ± SD",
    "Bytes allocated: min / mean ± SD",
    "Reads: min / mean ± SD",
    "Writes: min / mean ± SD",
    "Received bytes: min / mean ± SD",
    "Transmitted bytes: min / mean ± SD",
    "IO counter level",
];
fn cell(value: Option<&Option<Stats>>) -> Value {
    value.and_then(|v| v.as_ref()).map_or(Value::Null, |s| {
        json!(format!("{:.3} / {:.3} ± {:.3}", s.min, s.mean, s.sd))
    })
}
pub fn table_rows(summary: &Summary) -> Vec<Value> {
    summary
        .rows
        .iter()
        .map(|row| {
            json!([
                row.cell.runtime,
                row.cell.transport,
                row.cell.driver,
                row.cell.case,
                summary.repetitions,
                cell(row.metrics.get("wall_ns")),
                cell(row.metrics.get("cpu_ns")),
                cell(row.metrics.get("cycles")),
                cell(row.metrics.get("blocks")),
                cell(row.metrics.get("bytes")),
                cell(row.metrics.get("reads")),
                cell(row.metrics.get("writes")),
                cell(row.metrics.get("received_bytes")),
                cell(row.metrics.get("transmitted_bytes")),
                row.cell.io_level()
            ])
        })
        .collect()
}
fn markdown(summary: &Summary) -> String {
    let mut text=format!("# Runtime matrix ({})\n\nStatus: {}. Both loopback peers share one pinned runtime thread. Setup, warmup and shutdown are excluded. Each metric reports its independent minimum / mean ± sample SD across {} process runs; minima need not come from the same run. Concurrent/deep operations are whole batches. CPU cycles exclude kernel work and are unavailable when perf denies access. TLS counters measure ciphertext completions; WebTransport counters measure RPC stream completions, not packets or syscalls.\n\n",summary.mode,summary.status,summary.repetitions);
    if summary.mode == "smoke" {
        text.push_str("**Correctness smoke only; these are not performance measurements.**\n\n");
    }
    if let Some(error) = &summary.error {
        text.push_str(&format!("Failure: {error}\n\n"));
    }
    for (runtime, reason) in &summary.unsupported {
        text.push_str(&format!(
            "Unsupported {runtime}: {reason}. No fallback was used.\n\n"
        ));
    }
    text.push_str(&format!(
        "| {} |\n|{}|\n",
        COLUMNS.join(" | "),
        vec![" --- "; COLUMNS.len()].join("|")
    ));
    for row in table_rows(summary) {
        text.push_str(&format!(
            "| {} |\n",
            row.as_array()
                .unwrap()
                .iter()
                .map(|v| v
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| if v.is_null() {
                        "—".into()
                    } else {
                        v.to_string()
                    }))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    text
}
#[cfg(test)]
mod tests;
