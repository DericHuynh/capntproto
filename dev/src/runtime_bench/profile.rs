//! Measured-region instructions and allocations for the one supported stack.
use super::*;

fn counters(text: &str) -> Result<BTreeMap<String, u64>> {
    let events = text
        .lines()
        .find_map(|line| line.strip_prefix("events: "))
        .context("Callgrind events are missing")?
        .split_whitespace()
        .collect::<Vec<_>>();
    let counts = text
        .lines()
        .find_map(|line| line.strip_prefix("summary: "))
        .context("Callgrind measured-region summary is missing")?
        .split_whitespace()
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        events.len() == counts.len(),
        "incomplete Callgrind counters"
    );
    let result = events
        .into_iter()
        .map(str::to_owned)
        .zip(counts)
        .collect::<BTreeMap<_, _>>();
    for event in ["Ir", "Bc", "Bcm", "Bi", "Bim"] {
        ensure!(result.contains_key(event), "missing {event} counter");
    }
    ensure!(result["Ir"] > 0, "empty measured region");
    Ok(result)
}

pub(super) fn run(args: Args) -> Result<()> {
    ensure!(
        !args.output.exists(),
        "profile output must be a fresh directory"
    );
    fs::create_dir_all(&args.output)?;
    let worker = args.worker.canonicalize()?;
    let certificates = args.certificates.canonicalize()?;
    let cancel = Cancellation::install()?;
    let placement = pin_cpu()?;
    let mut rows = Vec::new();
    for mut cell in plan(false) {
        cell.iterations = if cell.case == "deep-32-concurrent-32" {
            2
        } else {
            16
        };
        cell.warmup = 8;
        let mut metrics: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for repetition in 0..5 {
            let base = args.output.join(format!("{}-{repetition}", cell.case));
            let trace = base.with_extension("callgrind");
            let output = crate::process::run(
                Command::new("valgrind")
                    .args(["--tool=callgrind", "--instr-atstart=no", "--branch-sim=yes"])
                    .arg(format!("--callgrind-out-file={}", trace.display()))
                    .arg(&worker)
                    .args([
                        "--case",
                        &cell.case,
                        "--iterations",
                        &cell.iterations.to_string(),
                        "--warmup",
                        &cell.warmup.to_string(),
                        "--certificates",
                    ])
                    .arg(&certificates),
                300,
                None,
                &cancel,
            )?;
            crate::write(base.with_extension("json"), &output.stdout)?;
            crate::write(base.with_extension("log"), &output.stderr)?;
            ensure!(
                output.code == 0,
                "{} profile failed: {}",
                cell.case,
                String::from_utf8_lossy(&output.stderr)
            );
            let value: Value = serde_json::from_slice(&output.stdout)?;
            ensure!(
                value["runtime"] == "tokio"
                    && value["driver"] == "legacy"
                    && value["transport"] == "tls"
                    && value["case"] == cell.case
                    && value["iterations"] == cell.iterations
                    && value["warmup"] == cell.warmup,
                "profile worker identity or workload mismatch"
            );
            ensure!(
                value["sample"]["callgrind_markers"] == true,
                "rebuild with CAPNTPROTO_VALGRIND_INCLUDE pointing to Valgrind headers"
            );
            for (event, count) in
                counters(&fs::read_to_string(trace.with_extension("callgrind.1"))?)?
            {
                metrics
                    .entry(event)
                    .or_default()
                    .push(count as f64 / cell.iterations as f64);
            }
            for metric in [
                "blocks",
                "bytes",
                "reads",
                "writes",
                "received_bytes",
                "transmitted_bytes",
            ] {
                let count = value["sample"][metric]
                    .as_u64()
                    .with_context(|| format!("missing {metric}"))?;
                metrics
                    .entry(metric.into())
                    .or_default()
                    .push(count as f64 / cell.iterations as f64);
            }
        }
        rows.push(
            json!({"case":cell.case,"iterations":cell.iterations,"warmup":cell.warmup,
            "metrics":metrics.into_iter().map(|(k,v)|(k,stats(&v))).collect::<BTreeMap<_,_>>() }),
        );
        println!("Profiled {} five times", cell.case);
    }
    let certificate_hashes = ["ca.der", "server.der"]
        .into_iter()
        .filter_map(|name| {
            let path = certificates.join(name);
            path.is_file().then_some((name, path))
        })
        .map(|(name, path)| Ok((name, crate::file_hash(path)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let report = json!({"version":2,"mode":"callgrind","runtime":"tokio","transport":"tls","driver":"legacy",
        "repetitions":5,"placement":placement,"worker_sha256":crate::file_hash(&worker)?,
        "source_diff_sha256":crate::hash(crate::process::text(Command::new("git").args(["diff", "--binary", "HEAD"]))?),
        "lockfile_sha256":crate::file_hash(crate::root().join("benchmarks/runtimes/Cargo.lock"))?,
        "valgrind":crate::process::text(Command::new("valgrind").arg("--version"))?.trim(),
        "certificates":certificate_hashes,
        "compiler":crate::process::text(Command::new("rustc").arg("-vV"))?,
        "source_commit":crate::process::text(Command::new("git").args(["rev-parse","HEAD"]))?.trim(),
        "dirty":!crate::process::text(Command::new("git").args(["status","--porcelain"]))?.is_empty(),
        "statistics":"independent minima, arithmetic mean, sample SD, maxima; per complete workload",
        "counter_model":"guest instructions and simulated conditional/indirect branch misses, not native cycles or latency",
        "rows":rows});
    crate::write_json(args.output.join("profiles.json"), &report)?;
    let mut text = String::from("# Legacy RPC / Tokio / rustls\n\nFive processes per case; setup, warmup and shutdown are excluded. Counters cover both loopback peers. Ir/Bcm/Bim are Callgrind's instruction/branch simulation; allocations and ciphertext IO come from the worker. Cells show independent minimum / mean ± sample SD. Concurrent cases report an entire batch.\n\n| Case | Ir | Bcm | Bim | Allocations | Allocated bytes | Reads | Writes |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for row in report["rows"].as_array().context("profile rows")? {
        text.push_str(&format!("| {} |", row["case"].as_str().context("case")?));
        for metric in ["Ir", "Bcm", "Bim", "blocks", "bytes", "reads", "writes"] {
            let s: Stats = serde_json::from_value(row["metrics"][metric].clone())?;
            text.push_str(&format!(" {:.2} / {:.2} ± {:.2} |", s.min, s.mean, s.sd));
        }
        text.push('\n');
    }
    crate::write(args.output.join("README.md"), text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parsing_requires_the_complete_nonempty_measured_region() {
        let valid = "events: Ir Bc Bcm Bi Bim\nsummary: 100 20 3 4 1\n";
        assert_eq!(counters(valid).unwrap()["Ir"], 100);
        for bad in [
            "",
            "events: Ir\nsummary: 100\n",
            "events: Ir Bc Bcm Bi Bim\nsummary: 0 1 0 0 0\n",
            "events: Ir Bc Bcm Bi Bim\nsummary: 100 1\n",
        ] {
            assert!(counters(bad).is_err());
        }
    }
}
