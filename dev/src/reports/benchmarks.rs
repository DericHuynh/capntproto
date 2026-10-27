//! Bounded, source-bound benchmark tables for the public reports template.
use super::data;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path};

pub(super) fn validate(tables: &Value) -> Result<()> {
    let tables = tables.as_array().context("invalid benchmark tables")?;
    ensure!(tables.len() <= 8, "too many benchmark tables");
    let mut names = BTreeSet::new();
    let mut total = 0;
    for table in tables {
        let object = table.as_object().context("invalid benchmark table")?;
        ensure!(
            object.len() == 3
                && ["title", "columns", "rows"]
                    .iter()
                    .all(|k| object.contains_key(*k)),
            "invalid benchmark table fields"
        );
        let title = crate::string(table, "title")?;
        ensure!(
            !title.is_empty() && title.len() <= 160 && names.insert(title),
            "invalid/duplicate benchmark table title"
        );
        let columns = table["columns"]
            .as_array()
            .context("invalid benchmark columns")?;
        ensure!(
            !columns.is_empty()
                && columns.len() <= 24
                && columns
                    .iter()
                    .all(|c| c.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 100)),
            "invalid benchmark columns"
        );
        let rows = table["rows"].as_array().context("invalid benchmark rows")?;
        total += rows.len();
        ensure!(
            rows.len() <= 4096 && total <= 8192,
            "too many benchmark rows"
        );
        for row in rows {
            let cells = row.as_array().context("invalid benchmark row")?;
            ensure!(cells.len() == columns.len(), "benchmark row width mismatch");
            for cell in cells {
                ensure!(
                    cell.is_null()
                        || cell.as_str().is_some_and(|s| s.len() <= 512)
                        || cell
                            .as_f64()
                            .is_some_and(|n| n.is_finite() && n.abs() <= 1e30),
                    "invalid benchmark cell"
                );
            }
        }
    }
    Ok(())
}
fn table(title: &str, columns: &[&str], rows: Vec<Value>) -> Value {
    json!({"title":title,"columns":columns,"rows":rows})
}
fn number(value: &Value) -> Result<f64> {
    let n = value.as_f64().context("missing benchmark number")?;
    ensure!(n.is_finite() && n.abs() <= 1e30, "invalid benchmark number");
    Ok(n)
}
fn micros(value: &Value) -> Result<Value> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    let n = number(value)?;
    ensure!(n >= 0., "negative benchmark latency");
    Ok(json!(n / 1000.))
}
fn latency_row(transport: &str, case: &Value, implementation: &str, cell: &Value) -> Result<Value> {
    if let Some(min) = cell.get("min_ns") {
        ensure!(number(min)? > 0., "invalid minimum latency");
    }
    let range = &cell["repetition_mean_range_ns"];
    Ok(json!([
        transport,
        case,
        implementation,
        cell["samples"],
        micros(&cell["min_ns"])?,
        micros(&cell["mean_ns"])?,
        micros(&cell["stddev_ns"])?,
        micros(&cell["repetition_mean_stddev_ns"])?,
        micros(&range[0])?,
        micros(&range[1])?,
        micros(&cell["p50_ns"])?,
        micros(&cell["p95_ns"])?,
        micros(&cell["p99_ns"])?
    ]))
}
const LATENCY_COLUMNS: &[&str] = &[
    "Transport",
    "Case",
    "Implementation",
    "Samples",
    "Minimum µs",
    "Mean µs",
    "Sample SD µs",
    "Run mean SD µs",
    "Run mean min µs",
    "Run mean max µs",
    "p50 µs",
    "p95 µs",
    "p99 µs",
];

pub(super) fn collect(
    directory: &Path,
    manifest: Option<&Path>,
    mut result: Value,
) -> Result<Value> {
    let mut tables = Vec::new();
    let evidence = directory.join("benchmark/evidence.json");
    if evidence.exists() && result["status"] == "passed" {
        let evidence = crate::read_json(evidence)?;
        let mut rows = Vec::new();
        for cell in evidence["data"]["rows"].as_array().into_iter().flatten() {
            let protocol = crate::string(cell, "protocol")?;
            let implementation = if protocol == "tcp" {
                format!(
                    "tcp / {}",
                    evidence["data"]["environment"]["tcp_driver"]
                        .as_str()
                        .unwrap_or("unrecorded")
                )
            } else {
                protocol.to_owned()
            };
            let mut cell = cell.clone();
            if cell["samples"].is_null() {
                if let (Some(a), Some(b)) = (
                    cell["repetitions"].as_u64(),
                    cell["iterations_per_repetition"].as_u64(),
                ) {
                    cell["samples"] = json!(a.checked_mul(b).context("sample count overflow")?);
                }
            }
            rows.push(latency_row(
                "recorded fixture",
                &json!(format!("{} bytes", crate::number(&cell, "payload_bytes")?)),
                &implementation,
                &cell,
            )?);
        }
        if !rows.is_empty() {
            tables.push(table("Loopback payload comparisons", LATENCY_COLUMNS, rows));
        }
    }
    // Latency-only campaigns deliberately have no old quality evidence.json.
    // Their independently validated native checkpoint is sufficient to report
    // measured latency even when later diagnostics or another CI stage fail.
    let analysis = directory.join("analysis");
    let report = [
        analysis.join("summary.json"),
        analysis.join("native-summary.json"),
    ]
    .into_iter()
    .find(|p| p.is_file());
    if let Some(path) = report {
        let report = crate::read_json(path)?;
        let manifest = crate::read_json(
            manifest.context("encrypted benchmark tables require the build manifest")?,
        )?;
        let source = crate::string(&manifest, "source_id")?;
        ensure!(
            data::matches(r"^[a-f0-9]{64}$", source),
            "invalid benchmark source fingerprint"
        );
        ensure!(
            result["source_id"].is_null() || result["source_id"] == source,
            "benchmark campaigns have different source fingerprints"
        );
        ensure!(
            matches!(
                report["measurement_outcome"].as_str(),
                Some("success" | "failure")
            ),
            "missing benchmark measurement outcome"
        );
        ensure!(
            data::matches(r"^[a-f0-9]{40}$", crate::string(&report["run"], "headSha")?),
            "invalid measured commit"
        );
        if !result["origin"].is_null() {
            let origin = &result["origin"];
            ensure!(
                report["run"]["headSha"] == origin["commit"]
                    && report["run"]["url"]
                        == format!(
                            "https://github.com/{}/actions/runs/{}",
                            crate::string(origin, "repository")?,
                            crate::number(origin, "run_id")?
                        ),
                "stale encrypted benchmark identity"
            );
        }
        let binaries = report["binaries"]
            .as_object()
            .context("missing measured executables")?;
        ensure!(!binaries.is_empty(), "missing measured executables");
        for (label, binary) in binaries {
            let hash = crate::string(binary, "sha256")?;
            ensure!(
                data::matches(r"^[a-f0-9]{64}$", hash),
                "invalid measured executable hash"
            );
            let name = match label.as_str() {
                "legacy" | "portable" | "grpc" | "websocket" => Some("driver"),
                "cpp" => Some("capnp-reference"),
                "upstream" => Some("capnp-rust"),
                _ => None,
            };
            if let Some(name) = name {
                ensure!(
                    manifest["binaries"][name] == hash,
                    "benchmark executable differs from build manifest"
                );
            }
        }
        result["source_id"] = json!(source);
        result["status"] = json!(if report["measurement_outcome"] == "success" {
            "passed"
        } else {
            "failed"
        });
        if result["status"] != "passed" {
            result["charts"] = json!([]);
        }
        result["note"] = json!("Independently validated encrypted benchmark values. Workflow status and diagnostic completion are separate; unavailable values remain blank. Operations include whole concurrent batches and every service hop.");
        let mut rows = Vec::new();
        let mut identities = BTreeSet::new();
        for row in report["latency"].as_array().into_iter().flatten() {
            let transport = crate::string(row, "transport")?;
            ensure!(
                ["tls", "quic"].contains(&transport),
                "invalid benchmark transport"
            );
            let case = crate::string(&row["case"], "name")?;
            for (label, cell) in row["variants"]
                .as_object()
                .context("missing benchmark variants")?
            {
                ensure!(
                    binaries.contains_key(label),
                    "unrecorded benchmark executable"
                );
                ensure!(
                    identities.insert((transport.to_owned(), case.to_owned(), label.to_owned())),
                    "duplicate benchmark cell"
                );
                rows.push(latency_row(transport, &json!(case), label, cell)?);
            }
        }
        if !rows.is_empty() {
            tables.push(table(
                "All encrypted latency comparisons",
                LATENCY_COLUMNS,
                rows,
            ));
        }
        let mut rows = Vec::new();
        for profile in report["profiles"].as_array().into_iter().flatten() {
            for (event, value) in profile["per_operation"]
                .as_object()
                .context("invalid profile counters")?
            {
                number(value)?;
                rows.push(json!([
                    profile["transport"],
                    profile["case"],
                    profile["variant"],
                    profile["mode"],
                    event,
                    value
                ]));
            }
        }
        if !rows.is_empty() {
            tables.push(table(
                "Instruction, branch and allocation counts per operation or batch",
                &[
                    "Transport",
                    "Case",
                    "Implementation",
                    "Profiler",
                    "Counter",
                    "Events per operation",
                ],
                rows,
            ));
        }
        let mut rows = Vec::new();
        for trace in report["traces"].as_array().into_iter().flatten() {
            for (event, counts) in trace["counts"]
                .as_object()
                .context("invalid syscall counters")?
            {
                number(&counts["mean"])?;
                number(&counts["sd"])?;
                rows.push(json!([
                    trace["transport"],
                    trace["case"],
                    trace["variant"],
                    trace["iterations"],
                    event,
                    counts["mean"],
                    counts["sd"]
                ]));
            }
        }
        if !rows.is_empty() {
            tables.push(table(
                "Whole-process syscall counts including setup",
                &[
                    "Transport",
                    "Case",
                    "Implementation",
                    "Operations",
                    "Counter",
                    "Mean",
                    "Run SD",
                ],
                rows,
            ));
        }
        let scope = directory.join("benchmark/secure/secure/native/scope.json");
        if scope.is_file() {
            let scope = crate::read_json(scope)?;
            ensure!(
                scope["binaries"] == report["binaries"],
                "stale unsupported benchmark scope"
            );
            let rows = scope["unsupported"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|row| {
                    json!([
                        row["transport"],
                        row["case"],
                        row["implementation"],
                        "Unsupported by this fixture/protocol"
                    ])
                })
                .collect::<Vec<_>>();
            if !rows.is_empty() {
                tables.push(table(
                    "Unsupported comparison cells",
                    &["Transport", "Case", "Implementation", "Status"],
                    rows,
                ));
            }
        }
    }
    let remote = directory.join("benchmark/remote/summary.json");
    if remote.is_file() && !result["source_id"].is_null() {
        let remote = crate::read_json(remote)?;
        ensure!(
            remote["validated"] == true,
            "unvalidated remote benchmark matrix"
        );
        let mut rows = Vec::new();
        for row in remote["rows"]
            .as_array()
            .context("missing remote benchmark rows")?
        {
            rows.push(latency_row(
                "TLS / two hosts",
                &row["case"]["name"],
                crate::string(row, "implementation")?,
                row,
            )?);
        }
        if !rows.is_empty() {
            tables.push(table(
                "All two-host application workflows",
                LATENCY_COLUMNS,
                rows,
            ));
        }
    }
    if !tables.is_empty() {
        result["tables"] = json!(tables);
        validate(&result["tables"])?;
    }
    Ok(result)
}

pub(super) fn markdown(data: &Value) -> Result<String> {
    let Some(tables) = data.get("tables") else {
        return Ok("No validated benchmark tables were produced for this run. See its original artifacts; unavailable data is not zero.\n".into());
    };
    validate(tables)?;
    let mut text = String::from("All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.\n\n");
    for table in tables.as_array().unwrap() {
        text.push_str(&format!(
            "### {}\n\n",
            super::render::markdown_text(crate::string(table, "title")?)
        ));
        let columns = table["columns"].as_array().unwrap();
        text.push('|');
        for column in columns {
            text.push_str(&format!(
                " {} |",
                super::render::markdown_text(column.as_str().unwrap())
            ));
        }
        text.push_str("\n|");
        for _ in columns {
            text.push_str(" --- |");
        }
        text.push('\n');
        for row in table["rows"].as_array().unwrap() {
            text.push('|');
            for value in row.as_array().unwrap() {
                let value = if value.is_null() {
                    "—".into()
                } else if let Some(s) = value.as_str() {
                    super::render::markdown_text(s)
                } else if let Some(n) = value.as_u64() {
                    n.to_string()
                } else {
                    format!("{:.3}", number(value)?)
                };
                text.push_str(&format!(" {value} |"));
            }
            text.push('\n');
        }
        text.push('\n');
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Value, Value) {
        let directory = tempfile::tempdir().unwrap();
        let manifest = json!({"source_id":"a".repeat(64),"binaries":{"driver":"b".repeat(64),"capnp-reference":"c".repeat(64)}});
        let cell = json!({"samples":5000,"min_ns":1500,"mean_ns":2400.,"stddev_ns":350.,"repetition_mean_stddev_ns":120.,"repetition_mean_range_ns":[2200.,2600.],"p50_ns":2300,"p95_ns":3000,"p99_ns":4100});
        let report = json!({"run":{"headSha":"d".repeat(40),"url":"https://github.com/example/capntproto/actions/runs/42"},"measurement_outcome":"success", "binaries":{"legacy":{"sha256":"b".repeat(64)},"portable":{"sha256":"b".repeat(64)},"cpp":{"sha256":"c".repeat(64)},"grpc":{"sha256":"b".repeat(64)},"websocket":{"sha256":"b".repeat(64)}},"latency":[{"transport":"tls","case":{"name":"deep-32-concurrent-32"},"variants":{"legacy":cell,"portable":cell,"cpp":cell,"grpc":cell,"websocket":cell}}], "profiles":[{"transport":"tls","case":"unary-small","variant":"portable","mode":"callgrind","per_operation":{"Ir":46202.85,"Bcm":326.65,"Bim":187.05}}],"traces":[{"transport":"tls","case":"unary-small","variant":"legacy","iterations":100,"counts":{"read":{"mean":43.,"sd":0.5},"write":{"mean":29.,"sd":0.}}}]});
        crate::write_json(directory.path().join("manifest.json"), &manifest).unwrap();
        (directory, manifest, report)
    }
    fn collect_report(path: &Path, report: &Value) -> Result<Value> {
        crate::write_json(path.join("analysis/summary.json"), report)?;
        let mut base = data::unavailable("benchmark", "No old-format evidence.");
        base["origin"] = json!({"repository":"example/capntproto","run_id":42,"attempt":1,"commit":"d".repeat(40)});
        collect(path, Some(&path.join("manifest.json")), base)
    }
    #[test]
    fn encrypted_campaign_without_old_evidence_retains_every_measurement() {
        let (directory, _, report) = fixture();
        let path = directory.path();
        crate::write_json(path.join("benchmark/secure/secure/native/scope.json"),
            &json!({"binaries":report["binaries"],"unsupported":[{"transport":"quic","case":"deep-32-concurrent-32","implementation":"grpc"},{"transport":"tls","case":"pipelined-read","implementation":"websocket"}]})).unwrap();
        let result = collect_report(path, &report).unwrap();
        data::validate_publication(&result).unwrap();
        assert_eq!(result["status"], "passed");
        let tables = result["tables"].as_array().unwrap();
        assert_eq!(tables.len(), 4);
        assert_eq!(tables[0]["rows"].as_array().unwrap().len(), 5);
        assert_eq!(tables[0]["rows"][0][4], 1.5);
        assert_eq!(tables[0]["rows"][0][12], 4.1);
        assert_eq!(tables[1]["rows"].as_array().unwrap().len(), 3);
        assert_eq!(tables[2]["rows"].as_array().unwrap().len(), 2);
        assert_eq!(tables[3]["rows"].as_array().unwrap().len(), 2);
        let text = markdown(&result).unwrap();
        for label in [
            "legacy",
            "portable",
            "cpp",
            "grpc",
            "websocket",
            "deep-32-concurrent-32",
            "Bcm",
            "Bim",
            "46202.850",
            "Unsupported",
            "read",
            "write",
        ] {
            assert!(
                text.contains(&super::super::render::markdown_text(label)) || text.contains(label),
                "missing {label}"
            );
        }
    }
    #[test]
    fn partial_diagnostics_preserve_independently_validated_native_checkpoint() {
        let (directory, _, mut report) = fixture();
        report.as_object_mut().unwrap().remove("profiles");
        report.as_object_mut().unwrap().remove("traces");
        report["measurement_outcome"] = json!("failure");
        crate::write_json(
            directory.path().join("analysis/native-summary.json"),
            &report,
        )
        .unwrap();
        let result = collect(
            directory.path(),
            Some(&directory.path().join("manifest.json")),
            data::unavailable("benchmark", "No old evidence"),
        )
        .unwrap();
        data::validate_publication(&result).unwrap();
        assert_eq!(result["status"], "failed");
        assert_eq!(result["tables"].as_array().unwrap().len(), 1);
        assert_eq!(result["tables"][0]["rows"].as_array().unwrap().len(), 5);
        assert!(result["charts"].as_array().unwrap().is_empty());
    }
    #[test]
    fn mixed_sources_executables_commits_and_duplicate_cells_are_rejected() {
        let (directory, _, report) = fixture();
        for (pointer, value) in [
            ("/run/headSha", json!("e".repeat(40))),
            (
                "/run/url",
                json!("https://github.com/example/capntproto/actions/runs/41"),
            ),
            ("/binaries/portable/sha256", json!("e".repeat(64))),
            ("/latency/0/variants/grpc/min_ns", json!(-1)),
            ("/measurement_outcome", Value::Null),
        ] {
            let mut bad = report.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(
                collect_report(directory.path(), &bad).is_err(),
                "accepted {pointer}"
            );
        }
        let mut duplicate = report.clone();
        duplicate["latency"]
            .as_array_mut()
            .unwrap()
            .push(report["latency"][0].clone());
        assert!(collect_report(directory.path(), &duplicate).is_err());
        crate::write_json(directory.path().join("analysis/summary.json"), &report).unwrap();
        let mut base = data::unavailable("benchmark", "Fixture");
        base["source_id"] = json!("f".repeat(64));
        assert!(collect(
            directory.path(),
            Some(&directory.path().join("manifest.json")),
            base
        )
        .is_err());
        assert!(collect(
            directory.path(),
            None,
            data::unavailable("benchmark", "Fixture")
        )
        .is_err());
    }
    #[test]
    fn tables_escape_untrusted_markdown_and_keep_missing_values_distinct_from_zero() {
        let data = json!({"tables":[table("<script> | [title](x)", &["Name", "Value"], vec![json!(["a|b\n<img src=x>", null]),json!(["zero", 0])])]});
        let text = markdown(&data).unwrap();
        assert!(!text.contains("<script>") && !text.contains("<img"));
        assert!(text.contains("&#60;script&#62;") && text.contains("—") && text.contains(" 0 |"));
        assert!(!text.contains("a|b") && !text.contains("[title](x)"));
        let mut bad = data["tables"].clone();
        bad[0]["rows"][0] = json!(["wrong width"]);
        assert!(validate(&bad).is_err());
        let mut bad = data["tables"].clone();
        bad[0]["rows"][0][1] = json!({"html":"invalid"});
        assert!(validate(&bad).is_err());
    }
}
