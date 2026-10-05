use crate::{
    benchmark, charts,
    coverage::{self, Metric, Metrics, Summary},
    source_id, Evidence, Result,
};
use capntproto_test_support::verification::{self as v, root};
use serde_json::Value;
use std::{collections::BTreeMap, fmt::Write as _, fs, path::Path};

fn text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
        .replace('\n', " ")
}
fn percentage(metric: &Metric) -> String {
    metric
        .percent()
        .map(|p| format!("{p:.2}% ({}/{})", metric.covered, metric.count))
        .unwrap_or_else(|| "N/A (no mapped counters)".into())
}
pub fn validate_evidence(e: &Evidence, id: &str, directory: &Path) -> Result<()> {
    if e.format != 1
        || e.source_id != id
        || !e.passed
        || e.error.is_some()
        || e.steps.is_empty()
        || e.steps.iter().any(|s| !s.passed)
    {
        return Err(format!("{}: incomplete, failed or stale evidence", e.lane).into());
    }
    for step in &e.steps {
        if !step
            .name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || step.name.is_empty()
        {
            return Err("unsafe log path".into());
        }
        if v::sha256(fs::read(
            directory.join("logs").join(format!("{}.log", step.name)),
        )?) != step.log_sha256
        {
            return Err(format!("{}: log hash mismatch", step.name).into());
        }
    }
    Ok(())
}
fn validate_coverage(e: &Evidence, directory: &Path) -> Result<(Summary, Summary)> {
    let summary: Summary = serde_json::from_slice(&fs::read(directory.join("summary.json"))?)?;
    if summary.scope != coverage::SCOPE {
        return Err("coverage source scope differs; requalification required".into());
    }
    let raw = fs::read(directory.join("coverage.json"))?;
    if e.data["llvm_json_sha256"] != v::sha256(&raw) {
        return Err("LLVM export hash mismatch".into());
    }
    let measured = coverage::parse_export(&serde_json::from_slice(&raw)?, &root())?;
    if summary.files != coverage::inventory(&measured)?
        || summary.totals != coverage::totals(&summary.files)
    {
        return Err("coverage inventory/metrics/totals differ from source tree and LLVM".into());
    }
    let baseline_path = root().join("quality/coverage-baseline.json");
    let baseline: Summary = serde_json::from_slice(&fs::read(&baseline_path).map_err(|_| {
        "coverage baseline missing; qualify and review the initial measurement first"
    })?)?;
    if baseline.scope != summary.scope {
        return Err("coverage baseline source scope differs; requalification required".into());
    }
    if baseline.totals != coverage::totals(&baseline.files) {
        return Err("baseline totals differ from its per-file counters".into());
    }
    if baseline.compiler != summary.compiler || baseline.flags != summary.flags {
        return Err("baseline compiler/options mismatch".into());
    }
    coverage::regression(&summary.files, &baseline.files)?;
    Ok((summary, baseline))
}
fn benchmark_rows(e: &Evidence, directory: &Path) -> Result<Vec<Value>> {
    let raw = fs::read(directory.join("trials.json"))?;
    if e.data["trials_sha256"] != v::sha256(&raw) {
        return Err("benchmark trial hash mismatch".into());
    }
    let trials: Vec<benchmark::Trial> = serde_json::from_slice(&raw)?;
    if trials.iter().any(|t| t.iterations < 1000 || t.warmup < 100) {
        return Err("benchmark below qualification budget".into());
    }
    let rows = benchmark::matrix(&trials)?;
    // Compare the stored representation on both sides. The default JSON float
    // parser can round a serialized f64 by one bit; integer counters remain exact.
    let stored_rows: Value = serde_json::from_slice(&serde_json::to_vec(&rows)?)?;
    if stored_rows != e.data["rows"] {
        return Err("benchmark summary differs from raw trials".into());
    }
    Ok(rows)
}
fn instruction_rows(e: &Evidence, directory: &Path) -> Result<Vec<crate::instructions::Row>> {
    let raw = fs::read(directory.join("instructions.jsonl"))?;
    if e.data["instructions_sha256"] != v::sha256(&raw) {
        return Err("instruction counter hash mismatch".into());
    }
    let rows = crate::instructions::parse(&raw)?;
    if serde_json::to_value(&rows)? != e.data["instructions"] {
        return Err("instruction summary differs from raw counters".into());
    }
    Ok(rows)
}
pub fn write_files(directory: &Path, summary: &Summary) -> Result<()> {
    let mut inventory = String::from("# First-party Rust source files\n\nN/A means no executable LLVM mapping in the tested builds, not 100% coverage. Schema/model/script files use their separate correctness gates.\n\n| File | Status | Lines | Regions | Functions | Branches |\n| --- | --- | --- | --- | --- | --- |\n");
    for (name, file) in &summary.files {
        let empty = Metrics::default();
        let m = file.metrics.as_ref().unwrap_or(&empty);
        writeln!(
            inventory,
            "| {} | {} | {} | {} | {} | {} |",
            text(name),
            text(&file.status),
            percentage(&m.lines),
            percentage(&m.regions),
            percentage(&m.functions),
            percentage(&m.branches)
        )?;
    }
    fs::write(directory.join("FILES.md"), inventory)?;
    Ok(())
}
pub fn generate(base: &Path, lanes: &str) -> Result<()> {
    fs::create_dir_all(base)?;
    let id = source_id()?;
    let expected: Vec<_> = lanes.split(',').map(str::to_owned).collect();
    if expected.is_empty()
        || expected.iter().any(|lane| {
            !["coverage", "qualification", "security", "benchmark"].contains(&lane.as_str())
        })
    {
        return Err("invalid required report lanes".into());
    }
    // Invalidate previous figures and README before reading the current evidence.
    charts::clear(base)?;
    fs::write(
        base.join("README.md"),
        "# Capntproto quality report\n\nReport generation incomplete.\n",
    )?;
    let mut figures = vec![];
    let mut evidence = BTreeMap::new();
    let mut failures = vec![];
    let mut readme = format!("# Capntproto quality report\n\nSource fingerprint: `{id}`. Measurement platform: Linux x86-64. Linux, macOS and Windows compile/smoke results are published by the separate CI workflow.\n\nThis README is generated from fresh test, LLVM and benchmark evidence. Missing or failed jobs prevent the required CI check from passing.\n\n## Required checks\n\n| Lane | Result |\n| --- | --- |\n");
    for lane in &expected {
        let directory = base.join(lane);
        let result: Result<Evidence> = (|| {
            let e: Evidence = serde_json::from_slice(&fs::read(directory.join("evidence.json"))?)?;
            if e.lane != *lane {
                return Err("artifact lane mismatch".into());
            }
            validate_evidence(&e, &id, &directory)?;
            Ok(e)
        })();
        match result {
            Ok(e) => {
                writeln!(readme, "| `{lane}` | Passed |")?;
                evidence.insert(lane.clone(), e);
            }
            Err(error) => {
                writeln!(readme, "| `{lane}` | Failed or missing |")?;
                failures.push(format!("{lane}: {error}"));
            }
        }
    }
    readme.push_str("\n## LLVM code coverage\n\nCoverage includes zero-hit mappings. Files without executable mappings are listed explicitly and are never counted as covered. Branch counters are instrumented; MC/DC is not claimed. Coverage is restricted to project-owned Rust sources; vendored dependencies, generated OUT_DIR files and the independent C++ reference are excluded.\n\n");
    if let Some(e) = evidence.get("coverage") {
        match validate_coverage(e, &base.join("coverage")) {
            Ok((summary, baseline)) => {
                figures.extend(charts::coverage(&summary, &baseline));
                readme.push_str("| Source group | Lines | Regions | Functions | Branches |\n| --- | --- | --- | --- | --- |\n");
                for (group, metrics) in &summary.totals {
                    writeln!(
                        readme,
                        "| {} | {} | {} | {} | {} |",
                        text(group),
                        percentage(&metrics.lines),
                        percentage(&metrics.regions),
                        percentage(&metrics.functions),
                        percentage(&metrics.branches)
                    )?;
                }
                let measured = summary
                    .files
                    .values()
                    .filter(|f| f.metrics.is_some())
                    .count();
                writeln!(readme,"\n{} source files inventoried; {measured} have LLVM mappings. Per-file and aggregate coverage must not regress against the reviewed baseline.\n\n[HTML coverage](coverage/html/index.html) · [LCOV](coverage/coverage.lcov) · [LLVM JSON](coverage/coverage.json) · [Every source file](coverage/FILES.md)\n", summary.files.len())?;
                write_files(&base.join("coverage"), &summary)?;
            }
            Err(error) => {
                writeln!(
                    readme,
                    "Coverage is not qualified: {}.\n",
                    text(&error.to_string())
                )?;
                failures.push(error.to_string());
            }
        }
    } else {
        readme
            .push_str("No validated coverage was supplied to this report; see the separate Verification / Coverage artifact. No percentage is reported.\n");
    }
    readme.push_str("\n## Linux loopback performance\n\nOne outstanding request, separate server/client processes, five repetitions per cell, rotated protocol order, validated sequence numbers and full payloads. Setup and warmup are excluded; latency includes serialization, transport, scheduling, and response validation. These are sequential round trips, not maximum concurrent throughput. Host load and CPU scheduling affect results.\n\n| Implementation | Transport in this run | Application contract |\n| --- | --- | --- |\n| Capntproto | Native QUIC v1 / TLS 1.3 / UDP with pinned peer authentication | Cap’n Proto capability RPC |\n| C++ Cap’n Proto | Plaintext TCP | Cap’n Proto capability RPC |\n| tonic gRPC | Plaintext HTTP/2 / TCP | Protobuf unary service RPC |\n| tokio-tungstenite | Plaintext WebSocket / TCP | Binary echo with an application sequence header |\n\nEncryption and protocol semantics differ. WebSockets alone do not supply the capability or RPC semantics of the other implementations. No result establishes a universal fastest protocol.\n\n");
    let mut instruction_counts = None;
    if let Some(e) = evidence.get("benchmark") {
        let results = benchmark_rows(e, &base.join("benchmark")).and_then(|rows| {
            instruction_counts = Some(instruction_rows(e, &base.join("benchmark"))?);
            Ok(rows)
        });
        match results {
            Ok(rows) => {
                figures.extend(charts::performance(&rows)?);
                readme.push_str("| Payload bytes | Implementation | p50 µs | p95 µs | p99 µs | Sequential req/s | p50 / Capntproto |\n| --- | --- | --- | --- | --- | --- | --- |\n");
                for row in &rows {
                    let native = rows
                        .iter()
                        .find(|r| {
                            r["protocol"] == "native" && r["payload_bytes"] == row["payload_bytes"]
                        })
                        .ok_or("missing Native comparison")?;
                    let p50 = row["p50_ns"].as_f64().ok_or("missing latency")?;
                    writeln!(
                        readme,
                        "| {} | {} | {:.2} | {:.2} | {:.2} | {:.0} | {:.4}× |",
                        row["payload_bytes"],
                        text(row["protocol"].as_str().unwrap()),
                        p50 / 1000.0,
                        row["p95_ns"].as_f64().unwrap() / 1000.0,
                        row["p99_ns"].as_f64().unwrap() / 1000.0,
                        row["sequential_requests_per_second"].as_f64().unwrap(),
                        p50 / native["p50_ns"].as_f64().unwrap()
                    )?;
                }
                readme.push_str("\nMeasurement version 2 includes bulk payload validation, response cleanup and ten-second per-call deadlines. Earlier unversioned comparisons used a slower C++ validation loop and are not comparable. The last column compares each implementation’s median latency with Capntproto at the same payload size; lower is less latency. [Raw per-request samples](benchmark/trials.json) and [environment, compiler/binary identities, repetition ranges and source pins](benchmark/evidence.json) accompany the results.\n");
            }
            Err(error) => {
                writeln!(
                    readme,
                    "Performance results are not qualified: {}.\n",
                    text(&error.to_string())
                )?;
                failures.push(error.to_string());
            }
        }
    } else {
        readme.push_str(
            "No validated performance was supplied to this report; see the separate Performance / Dedicated benchmarks artifact. No comparison numbers are reported.\n",
        );
    }
    if let Some(rows) = instruction_counts {
        readme.push_str("\n## Serialization instruction counts\n\n");
        figures.push(charts::instructions(&rows));
        readme.push_str("Gungraun 0.20 / Callgrind measures the maintained Rust serializer on the same dedicated host, after the loopback runs. Setup is excluded; validation and ownership cleanup remain in the measured functions. These counts do not compare RPC protocols or establish a historical regression baseline.\n\n| Operation | Payload bytes | Instructions | Data reads | Data writes |\n| --- | --- | --- | --- | --- |\n");
        for row in rows {
            writeln!(
                readme,
                "| {} | {} | {} | {} | {} |",
                row.operation, row.payload_bytes, row.instructions, row.data_reads, row.data_writes
            )?;
        }
        readme.push_str("\n[Raw Gungraun summaries](benchmark/instructions.jsonl) and profiler identity in [benchmark evidence](benchmark/evidence.json) accompany these counts.\n");
    }
    if !failures.is_empty() {
        readme.push_str("\n## Blocking failures\n\n");
        for failure in &failures {
            writeln!(readme, "- {}", text(failure))?;
        }
    }
    readme.push_str("\n## Scope\n\nThis is bounded automated evidence for the tested source and environment. It includes independent C++ comparisons, model checks, compiler contracts, Miri, sanitizers, implementation mutation tests, default-feature tests, provenance and known-advisory checks. It is not an independent cryptographic audit or an unbounded protocol proof. Ignored diagnostic/full-release tests retain their documented scope in docs/wiki/Testing.md and docs/wiki/Release-Acceptance.md.\n");
    charts::append(&mut readme, base, &id, &figures)?;
    fs::write(base.join("README.md"), readme)?;
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} quality requirements failed:\n- {}\nSee {}/README.md",
            failures.len(),
            failures.join("\n- "),
            base.display()
        )
        .into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persisted_benchmark_rounding_and_raw_sample_integrity() {
        let dir = tempfile::tempdir().unwrap();
        let mut trials = Vec::new();
        for bytes in benchmark::PAYLOADS {
            for protocol in benchmark::PROTOCOLS {
                for repetition in 0..5 {
                    trials.push(benchmark::Trial {
                        measurement_version: benchmark::MEASUREMENT_VERSION,
                        protocol: protocol.into(),
                        payload_bytes: bytes,
                        iterations: 1000,
                        warmup: 100,
                        elapsed_ns: if repetition == 4 {
                            129772832
                        } else {
                            129772828
                        },
                        latency_ns: vec![100; 1000],
                    });
                }
            }
        }
        // These real-run elapsed values reproduce the f64 JSON round-trip issue.
        let raw = serde_json::to_vec(&trials).unwrap();
        let rows = benchmark::matrix(&trials).unwrap();
        fs::write(dir.path().join("trials.json"), &raw).unwrap();
        let e = Evidence {
            format: 1,
            lane: "benchmark".into(),
            source_id: "test".into(),
            passed: true,
            error: None,
            compiler: "test".into(),
            steps: vec![],
            data: serde_json::json!({"rows": rows, "trials_sha256": v::sha256(&raw)}),
        };
        let mut stored: Evidence =
            serde_json::from_slice(&serde_json::to_vec(&e).unwrap()).unwrap();
        assert!(benchmark_rows(&stored, dir.path()).is_ok());
        stored.data["rows"][0]["p50_ns"] = serde_json::json!(101);
        assert!(benchmark_rows(&stored, dir.path()).is_err());
        let stored: Evidence = serde_json::from_slice(&serde_json::to_vec(&e).unwrap()).unwrap();
        // Altering one sample does not change any reported percentile. Its hash
        // must still reject the modification.
        trials[0].latency_ns[0] += 1;
        assert_eq!(benchmark::matrix(&trials).unwrap(), rows);
        fs::write(
            dir.path().join("trials.json"),
            serde_json::to_vec(&trials).unwrap(),
        )
        .unwrap();
        assert!(benchmark_rows(&stored, dir.path()).is_err());
    }
    #[test]
    fn report_appends_charts_and_removes_them_when_samples_are_tampered() {
        let dir = tempfile::tempdir().unwrap();
        let lane = dir.path().join("benchmark");
        fs::create_dir_all(lane.join("logs")).unwrap();
        fs::write(lane.join("logs/fixture.log"), "synthetic test fixture").unwrap();
        let mut trials = vec![];
        for bytes in benchmark::PAYLOADS {
            for (index, protocol) in benchmark::PROTOCOLS.iter().enumerate() {
                let latency = (index as u64 + 1) * 10_000 + bytes as u64;
                for _ in 0..5 {
                    trials.push(benchmark::Trial {
                        measurement_version: benchmark::MEASUREMENT_VERSION,
                        protocol: (*protocol).into(),
                        payload_bytes: bytes,
                        iterations: 1000,
                        warmup: 100,
                        elapsed_ns: latency * 1100,
                        latency_ns: vec![latency; 1000],
                    });
                }
            }
        }
        let raw = serde_json::to_vec(&trials).unwrap();
        fs::write(lane.join("trials.json"), &raw).unwrap();
        let instructions = crate::instructions::fixture()
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(lane.join("instructions.jsonl"), &instructions).unwrap();
        let evidence = Evidence {
            format: 1,
            lane: "benchmark".into(),
            source_id: source_id().unwrap(),
            passed: true,
            error: None,
            compiler: "synthetic test fixture".into(),
            steps: vec![crate::Step {
                name: "fixture".into(),
                command: vec![],
                seconds: 1.0,
                passed: true,
                log_sha256: v::sha256("synthetic test fixture"),
            }],
            data: serde_json::json!({"trials_sha256": v::sha256(&raw), "rows": benchmark::matrix(&trials).unwrap(),
                "instructions_sha256":v::sha256(instructions.as_bytes()),
                "instructions":crate::instructions::parse(instructions.as_bytes()).unwrap()}),
        };
        fs::write(
            lane.join("evidence.json"),
            serde_json::to_vec(&evidence).unwrap(),
        )
        .unwrap();
        generate(dir.path(), "benchmark").unwrap();
        let readme = fs::read_to_string(dir.path().join("README.md")).unwrap();
        assert!(
            readme.rfind("\n## Comparison bar charts").unwrap()
                > readme.rfind("\n## Scope").unwrap()
        );
        assert_eq!(readme.matches("](charts/").count(), 8); // data + seven SVGs
        let data: Value =
            serde_json::from_slice(&fs::read(dir.path().join("charts/data.json")).unwrap())
                .unwrap();
        assert_eq!(data["source_id"], evidence.source_id);
        assert_eq!(data["charts"][0]["panels"][0]["bars"][0]["value"], 10.0);
        fs::remove_file(lane.join("instructions.jsonl")).unwrap();
        assert!(generate(dir.path(), "benchmark").is_err());
        assert_eq!(fs::read_dir(dir.path().join("charts")).unwrap().count(), 0);
        fs::write(
            lane.join("instructions.jsonl"),
            instructions.replace("100", "101"),
        )
        .unwrap();
        assert!(generate(dir.path(), "benchmark").is_err());
        fs::write(lane.join("instructions.jsonl"), &instructions).unwrap();
        generate(dir.path(), "benchmark").unwrap();
        // Alter a sample without changing a percentile: raw integrity still blocks every chart.
        trials[0].latency_ns[0] += 1;
        fs::write(
            lane.join("trials.json"),
            serde_json::to_vec(&trials).unwrap(),
        )
        .unwrap();
        let error = generate(dir.path(), "benchmark").unwrap_err().to_string();
        assert!(error.contains("benchmark trial hash mismatch"), "{error}");
        assert!(error.contains("README.md"), "{error}");
        let readme = fs::read_to_string(dir.path().join("README.md")).unwrap();
        assert!(!readme.contains("!["));
        assert!(readme.contains("No validated coverage or benchmark measurements"));
        assert_eq!(fs::read_dir(dir.path().join("charts")).unwrap().count(), 0);
    }
    #[test]
    fn stale_failed_and_missing_results_do_not_certify_a_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = Evidence {
            format: 1,
            lane: "coverage".into(),
            source_id: "old".into(),
            passed: true,
            error: None,
            compiler: "test".into(),
            steps: vec![],
            data: serde_json::json!({}),
        };
        assert!(validate_evidence(&e, "new", dir.path()).is_err());
        e.source_id = "new".into();
        assert!(validate_evidence(&e, "new", dir.path()).is_err());
        e.passed = false;
        assert!(validate_evidence(&e, "new", dir.path()).is_err());
        fs::create_dir(dir.path().join("logs")).unwrap();
        fs::write(dir.path().join("logs/test.log"), "passed").unwrap();
        e.steps.push(crate::Step {
            name: "test".into(),
            command: vec!["cargo".into(), "test".into()],
            seconds: 1.0,
            passed: true,
            log_sha256: v::sha256("passed"),
        });
        e.passed = true;
        assert!(validate_evidence(&e, "new", dir.path()).is_ok());
        fs::write(dir.path().join("logs/test.log"), "tampered").unwrap();
        assert!(validate_evidence(&e, "new", dir.path()).is_err());
        assert_eq!(text("<x>|\n"), "&lt;x&gt;&#124; ");
    }
}
