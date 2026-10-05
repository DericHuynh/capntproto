use super::data::{matches, validate_publication, CHARTS};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
const LANES: [&str; 5] = ["full", "cargo", "models", "fuzz", "benchmark"];
const COLORS: [&str; 5] = ["#0868d4", "#00ac7b", "#ef4444", "#e89200", "#8b5cf6"];
fn esc(s: &str) -> String {
    html_escape::encode_safe(s).into_owned()
}
/// Artifact labels are text, never Markdown links, images or raw HTML.
fn markdown_text(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_punctuation() {
                format!("&#{};", u32::from(c))
            } else if matches!(c, '\r' | '\n') {
                " ".into()
            } else {
                c.to_string()
            }
        })
        .collect()
}
pub fn empty_history() -> Value {
    json!({"format":1,"full":[],"cargo":[],"models":[],"fuzz":[],"benchmark":null})
}
pub fn validate_record(record: &Value) -> Result<()> {
    let fields = [
        "run_id",
        "attempt",
        "commit",
        "date",
        "url",
        "conclusion",
        "data",
    ];
    let o = record.as_object().context("invalid history record")?;
    ensure!(
        o.len() == fields.len() && fields.iter().all(|k| o.contains_key(*k)),
        "unexpected history fields"
    );
    ensure!(
        crate::number(record, "run_id")? > 0 && crate::number(record, "attempt")? > 0,
        "invalid run identity"
    );
    ensure!(
        matches(r"^[a-f0-9]{40}$", crate::string(record, "commit")?),
        "invalid source commit"
    );
    chrono::DateTime::parse_from_rfc3339(crate::string(record, "date")?)?;
    ensure!(
        matches(
            r"^https://github\.com/[\w.-]+/[\w.-]+/actions/runs/\d+$",
            crate::string(record, "url")?
        ),
        "invalid run URL"
    );
    ensure!(
        [
            "success",
            "failure",
            "cancelled",
            "timed_out",
            "action_required",
            "skipped",
            "neutral",
            "stale",
            "startup_failure"
        ]
        .contains(&crate::string(record, "conclusion")?),
        "invalid workflow conclusion"
    );
    validate_publication(&record["data"])
}
fn order(record: &Value) -> Result<(chrono::DateTime<chrono::FixedOffset>, u64, u64)> {
    Ok((
        chrono::DateTime::parse_from_rfc3339(crate::string(record, "date")?)?,
        crate::number(record, "run_id")?,
        crate::number(record, "attempt")?,
    ))
}
pub fn validate_history(history: &Value) -> Result<()> {
    let o = history.as_object().context("invalid history")?;
    ensure!(
        ["format", "full", "benchmark"]
            .iter()
            .all(|k| o.contains_key(*k))
            && o.keys().all(
                |k| ["format", "full", "benchmark", "cargo", "models", "fuzz"]
                    .contains(&k.as_str())
            )
            && history["format"] == 1,
        "invalid history format"
    );
    let mut identities = BTreeMap::new();
    for kind in LANES {
        let mut ids = BTreeSet::new();
        let records: Vec<&Value> = if kind == "benchmark" {
            (!history[kind].is_null())
                .then_some(&history[kind])
                .into_iter()
                .collect()
        } else if let Some(value) = history.get(kind) {
            value
                .as_array()
                .context("invalid history size")?
                .iter()
                .collect()
        } else {
            vec![]
        };
        ensure!(records.len() <= 365, "invalid history size");
        let mut previous = None;
        for record in records {
            validate_record(record)?;
            ensure!(
                ids.insert(crate::number(record, "run_id")?),
                "duplicate history run"
            );
            // A reusable CI run publishes several lanes. They share a run ID,
            // but must agree on the immutable source and run identity. Attempts
            // may differ when re-running only failed jobs.
            let identity = [&record["commit"], &record["date"], &record["url"]];
            if let Some(previous) = identities.insert(crate::number(record, "run_id")?, identity) {
                ensure!(previous == identity, "inconsistent cross-lane run identity");
            }
            ensure!(record["data"]["kind"] == kind, "invalid history lane");
            let current = order(record)?;
            ensure!(
                previous.is_none_or(|p| p <= current),
                "history is not chronological"
            );
            previous = Some(current);
        }
    }
    Ok(())
}
pub fn merge(history: &Value, record: &Value) -> Result<Value> {
    validate_history(history)?;
    validate_record(record)?;
    let mut history = history.clone();
    let kind = crate::string(&record["data"], "kind")?;
    if kind == "benchmark" {
        if history[kind].is_null() || order(record)? > order(&history[kind])? {
            history[kind] = record.clone();
        }
    } else {
        if history[kind].is_null() {
            history[kind] = json!([]);
        }
        let records = history[kind].as_array_mut().unwrap();
        if records.iter().any(|r| {
            r["run_id"] == record["run_id"] && r["attempt"].as_u64() >= record["attempt"].as_u64()
        }) {
            return Ok(history);
        }
        records.retain(|r| r["run_id"] != record["run_id"]);
        records.push(record.clone());
        records.sort_by_key(|r| order(r).expect("validated record"));
        if records.len() > 365 {
            records.drain(..records.len() - 365);
        }
    }
    for lane in ["full", "cargo", "models", "fuzz"] {
        if let Some(records) = history[lane].as_array_mut() {
            let end = records.len().saturating_sub(1);
            for old in &mut records[..end] {
                old["data"]["charts"] = json!([]);
                if let Some(tests) = old["data"]["tests"].as_object_mut() {
                    tests.remove("failures");
                }
            }
        }
    }
    validate_history(&history)?;
    Ok(history)
}
pub fn failure_details(data: &Value) -> Result<String> {
    validate_publication(data)?;
    if data["kind"] == "fuzz" {
        return Ok(format!("Fuzz campaign status: **{}**. {}\n\nSee the engine graphs and retained logs, corpus, crashes and hangs in this workflow artifact.\n",crate::string(data,"status")?,markdown_text(crate::string(data,"note")?)));
    }
    let tests = &data["tests"];
    if tests.is_null() {
        return Ok("No test inventory was produced. A build or infrastructure failure is not a passing test run. Inspect the workflow logs.\n".into());
    }
    let mut text = format!(
        "Recorded: **{} failed**, **{} without a terminal result**, {} passed and {} skipped.\n\n",
        tests["failed"], tests["errors"], tests["passed"], tests["skipped"]
    );
    let Some(failures) = tests.get("failures") else {
        return Ok(text+"This older report contains counts only. Failed test names and output are in the workflow artifacts.\n");
    };
    let failures = failures.as_array().unwrap();
    if failures.is_empty() {
        text.push_str("No completed test reported a failure.\n\n");
    }
    for (i, f) in failures.iter().enumerate() {
        text.push_str(&format!(
            "<details open>\n<summary>{}. <code>{}</code></summary>\n\n<p>{}</p>\n<pre>{}</pre>\n",
            i + 1,
            esc(crate::string(f, "name")?),
            esc(crate::string(f, "suite")?),
            esc(crate::string(f, "output")?)
        ));
        if f["truncated"] == true {
            text.push_str(
                "\nDiagnostic excerpt truncated; full output is in the workflow artifact.\n",
            );
        }
        text.push_str("\n</details>\n\n");
    }
    if tests["command_passed"] != true {
        text.push_str("The workspace command failed or was incomplete. Build/infrastructure errors and tests that never finished are not invented as named failures.\n");
    }
    Ok(text)
}
fn failure_report(record: Option<&Value>) -> Result<String> {
    let mut text = "# Failed workspace tests\n\n".to_owned();
    let Some(r) = record else {
        return Ok(text + "No run has been recorded for this partition.\n");
    };
    validate_record(r)?;
    text.push_str(&format!(
        "[Workflow run and full logs]({}) · commit `{}` · attempt {}\n\n",
        crate::string(r, "url")?,
        crate::string(r, "commit")?,
        r["attempt"]
    ));
    text.push_str(&failure_details(&r["data"])?);
    Ok(text)
}
fn text(svg: &mut String, x: f64, y: f64, size: u32, value: &str, color: &str) {
    svg.push_str(&format!(
        "<text x=\"{x:.2}\" y=\"{y:.2}\" font-size=\"{size}\" fill=\"{color}\">{}</text>\n",
        esc(value)
    ));
}
fn start(title: &str, height: usize) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1400\" height=\"{height}\" viewBox=\"0 0 1400 {height}\" role=\"img\" aria-labelledby=\"title\">\n<title id=\"title\">{}</title>\n<rect width=\"1400\" height=\"{height}\" fill=\"white\"/>\n<g font-family=\"DejaVu Sans, sans-serif\">\n",esc(title))
}
fn wrap(value: &str, limit: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in value.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > limit {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
fn finish(mut svg: String, path: &Path) -> Result<()> {
    svg.push_str("</g>\n</svg>\n");
    crate::write(path, svg)
}
pub fn bar_chart(chart: &Value, path: &Path, stamp: &str) -> Result<()> {
    let title = crate::string(chart, "title")?.replace("ReProto", "Capntproto");
    let note = crate::string(chart, "note")?.replace("ReProto", "Capntproto");
    let panels = chart["panels"].as_array().context("missing panels")?;
    let note_lines = wrap(&note, 145);
    let height = 180
        + note_lines.len() * 20
        + panels
            .iter()
            .map(|p| 100 + p["bars"].as_array().unwrap().len() * 42)
            .sum::<usize>();
    let mut svg = start(&title, height);
    text(&mut svg, 35.0, 44.0, 27, &title, "#1e293b");
    let mut y = 78.0;
    for line in note_lines {
        text(&mut svg, 35.0, y, 14, &line, "#64748b");
        y += 20.0;
    }
    y += 24.0;
    for panel in panels {
        text(
            &mut svg,
            35.0,
            y,
            20,
            crate::string(panel, "title")?,
            "#1e293b",
        );
        y += 36.0;
        let bars = panel["bars"].as_array().unwrap();
        let low = bars
            .iter()
            .filter_map(|b| b["value"].as_f64())
            .fold(0.0, f64::min);
        let high = bars
            .iter()
            .filter_map(|b| b["value"].as_f64())
            .fold(0.0, f64::max);
        let span = (high - low).max(1.0);
        let min = if low < 0.0 { low - span * 0.15 } else { 0.0 };
        let max = high + span * 0.2;
        let x = |v: f64| 600.0 + (v - min) / (max - min) * 720.0;
        for (i, bar) in bars.iter().enumerate() {
            let label = crate::string(bar, "label")?;
            let label = if chart["name"].as_str().unwrap_or("").starts_with("tla-") {
                label.into()
            } else {
                label.replace("ReProto", "Capntproto")
            };
            let chunks = wrap(&label, 67);
            for (j, line) in chunks.into_iter().take(2).enumerate() {
                text(
                    &mut svg,
                    35.0,
                    y - 5.0 + j as f64 * 15.0,
                    13,
                    &line,
                    "#334155",
                );
            }
            if let Some(value) = bar["value"].as_f64() {
                let pos = x(value);
                let zero = x(0.0);
                svg.push_str(&format!(
                    "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"23\" fill=\"{}\"/>\n",
                    pos.min(zero),
                    y - 20.0,
                    (pos - zero).abs(),
                    COLORS[i % COLORS.len()]
                ));
                let label = if chart["name"].as_str().unwrap_or("").ends_with("difference") {
                    format!("{value:+.1}%")
                } else if bar["value"].is_u64() {
                    bar["value"].to_string()
                } else {
                    format!("{value:.2}")
                };
                text(
                    &mut svg,
                    pos.max(zero) + 6.0,
                    y - 3.0,
                    13,
                    &label,
                    "#334155",
                );
            } else {
                text(&mut svg, 610.0, y - 3.0, 14, "N/A", "#64748b");
            }
            y += 42.0;
        }
        text(
            &mut svg,
            600.0,
            y,
            13,
            crate::string(chart, "unit")?,
            "#64748b",
        );
        y += 64.0;
    }
    for line in wrap(stamp, 145) {
        text(&mut svg, 35.0, y, 13, &line, "#64748b");
        y += 18.0;
    }
    finish(svg, path)
}
fn latest<'a>(history: &'a Value, kind: &str) -> Option<&'a Value> {
    if kind == "benchmark" {
        (!history[kind].is_null()).then_some(&history[kind])
    } else {
        history[kind].as_array().and_then(|a| a.last())
    }
}
fn test_chart(history: &Value, path: &Path, kind: &str, title: &str) -> Result<()> {
    let mut svg = start(title, 650);
    text(
        &mut svg,
        35.0,
        45.0,
        27,
        &format!("Capntproto — {title}"),
        "#1e293b",
    );
    text(
        &mut svg,
        35.0,
        78.0,
        15,
        "Nextest and doctest results · selected partition · counts per CI run · UTC",
        "#64748b",
    );
    let records: Vec<_> = history[kind]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| !r["data"]["tests"].is_null())
        .collect();
    for (i, (key, label)) in [
        ("total", "Total"),
        ("passed", "Pass"),
        ("failed", "Fail"),
        ("errors", "Error"),
        ("skipped", "Skip"),
    ]
    .into_iter()
    .enumerate()
    {
        text(
            &mut svg,
            70.0 + i as f64 * 150.0,
            115.0,
            16,
            label,
            COLORS[i],
        );
        if records.is_empty() {
            continue;
        }
        let max = records
            .iter()
            .filter_map(|r| r["data"]["tests"]["total"].as_u64())
            .max()
            .unwrap_or(1)
            .max(1) as f64;
        let first = order(records[0])?.0.timestamp_millis();
        let last = order(records[records.len() - 1])?.0.timestamp_millis();
        let x = |r: &Value| -> f64 {
            if first == last {
                700.0
            } else {
                80.0 + (order(r).unwrap().0.timestamp_millis() - first) as f64
                    / (last - first) as f64
                    * 1220.0
            }
        };
        let y = |r: &Value| 530.0 - r["data"]["tests"][key].as_u64().unwrap() as f64 / max * 350.0;
        let points = records
            .iter()
            .map(|r| format!("{:.2},{:.2}", x(r), y(r)))
            .collect::<Vec<_>>()
            .join(" ");
        svg.push_str(&format!(
            "<polyline points=\"{points}\" fill=\"none\" stroke=\"{}\" stroke-width=\"2.3\"/>\n",
            COLORS[i]
        ));
        for r in &records {
            svg.push_str(&format!("<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"3\" fill=\"{}\"><title>{}: {}</title></circle>\n",x(r),y(r),COLORS[i],esc(crate::string(r,"date")?),r["data"]["tests"][key]));
            if i == 0 && r["data"]["tests"]["complete"] != true {
                svg.push_str(&format!(
                    "<path d=\"M {:.2} 160 V 540\" stroke=\"#ef4444\" stroke-dasharray=\"2 5\"/>\n",
                    x(r)
                ));
            }
        }
        if i == 0 {
            for tick in 0..=4 {
                let value = max * tick as f64 / 4.0;
                text(
                    &mut svg,
                    10.0,
                    530.0 - 350.0 * tick as f64 / 4.0,
                    12,
                    &format!("{value:.0}"),
                    "#64748b",
                );
            }
            text(
                &mut svg,
                80.0,
                565.0,
                13,
                &order(records[0])?
                    .0
                    .format("%Y-%m-%d %H:%M UTC")
                    .to_string(),
                "#64748b",
            );
            text(
                &mut svg,
                1060.0,
                565.0,
                13,
                &order(records[records.len() - 1])?
                    .0
                    .format("%Y-%m-%d %H:%M UTC")
                    .to_string(),
                "#64748b",
            );
        }
    }
    let note = if latest(history, kind).is_some_and(|r| r["data"]["tests"].is_null()) {
        "LATEST RUN HAS NO TEST COUNTS — see its CI log. Earlier points are historical evidence."
    } else if records.is_empty() {
        text(
            &mut svg,
            200.0,
            330.0,
            23,
            "Awaiting the first CI measurement for this partition",
            "#64748b",
        );
        "No invented history. Missing measurements are not zero tests or passing tests."
    } else {
        "Dotted lines: failed/incomplete workspace command. Error = announced tests without a terminal result."
    };
    text(&mut svg, 35.0, 620.0, 13, note, "#64748b");
    finish(svg, path)
}
fn run_text(r: &Value) -> Result<String> {
    Ok(format!(
        "[{} · run {} / attempt {}]({}) · commit `{}` · **{}**",
        crate::string(r, "date")?,
        r["run_id"],
        r["attempt"],
        crate::string(r, "url")?,
        &crate::string(r, "commit")?[..12],
        crate::string(r, "conclusion")?
    ))
}
fn charts_text(r: &Value, assets: &Path) -> Result<String> {
    let mut text = String::new();
    for chart in r["data"]["charts"].as_array().context("missing charts")? {
        bar_chart(
            chart,
            &assets.join(format!("{}.svg", crate::string(chart, "name")?)),
            &format!(
                "Measured {} | commit {}",
                crate::string(r, "date")?,
                &crate::string(r, "commit")?[..12]
            ),
        )?;
        text.push_str(&format!(
            "![{}](docs/reports/{}.svg)\n\n{}\n\n",
            markdown_text(crate::string(chart, "title")?),
            crate::string(chart, "name")?,
            markdown_text(crate::string(chart, "note")?)
        ));
    }
    Ok(text)
}
pub fn artifact(data: &Value, output: &Path) -> Result<()> {
    validate_publication(data)?;
    std::fs::create_dir_all(output)?;
    for name in CHARTS.into_iter().chain(["test-results"]) {
        crate::remove(output.join(format!("{name}.svg")))?;
    }
    crate::remove(output.join("FAILED-TESTS.md"))?;
    let mut charts = data["charts"].as_array().unwrap().clone();
    let mut text = format!(
        "# {} verification\n\nStatus: **{}**. {}\n\n",
        crate::string(data, "kind")?,
        crate::string(data, "status")?,
        markdown_text(crate::string(data, "note")?)
    );
    if !data["tests"].is_null() {
        charts.insert(
            0,
            super::verification::chart(
                "test-results",
                "Test results",
                "Tests",
                "Actual selected nextest/doctest cases; filtered tests are not passes.",
                vec![(
                    "Results",
                    ["passed", "failed", "errors", "skipped"]
                        .into_iter()
                        .map(|k| (k.into(), data["tests"][k].clone()))
                        .collect(),
                )],
            ),
        );
        crate::write(output.join("FAILED-TESTS.md"), failure_details(data)?)?;
        text.push_str("[All failed tests](FAILED-TESTS.md).\n\n");
    }
    for chart in &charts {
        bar_chart(
            chart,
            &output.join(format!("{}.svg", crate::string(chart, "name")?)),
            &format!(
                "Source fingerprint: {}",
                data["source_id"].as_str().unwrap_or("unavailable")
            ),
        )?;
        text.push_str(&format!(
            "![{}]({}.svg)\n\n{}\n\n",
            markdown_text(crate::string(chart, "title")?),
            crate::string(chart, "name")?,
            markdown_text(crate::string(chart, "note")?)
        ));
    }
    if charts.is_empty() {
        text.push_str("No validated measurements were produced; inspect the job logs.\n");
    }
    crate::write(output.join("README.md"), text)
}
pub fn owned() -> BTreeSet<String> {
    CHARTS
        .into_iter()
        .map(|n| format!("docs/reports/{n}.svg"))
        .chain(
            [
                "README.md",
                "docs/reports/history.json",
                "docs/reports/test-history.svg",
                "docs/reports/cargo-history.svg",
                "docs/reports/tla-history.svg",
                "docs/reports/failed-models.md",
                "docs/reports/benchmarks-pending.svg",
                "docs/reports/failed-tests.md",
            ]
            .map(str::to_owned),
        )
        .collect()
}
pub fn render(template: &str, history: &Value, output: &Path) -> Result<Vec<PathBuf>> {
    validate_history(history)?;
    ensure!(
        template.matches("{{REPORTS}}").count() == 1,
        "README template must contain exactly one {{REPORTS}} slot"
    );
    let assets = output.join("docs/reports");
    std::fs::create_dir_all(&assets)?;
    for name in CHARTS.into_iter().chain(["benchmarks-pending"]) {
        crate::remove(assets.join(format!("{name}.svg")))?;
    }
    for (kind, name, title) in [
        ("full", "test-history", "Workspace Test Results"),
        ("cargo", "cargo-history", "Cargo Test Results"),
        ("models", "tla-history", "TLA+ Rust Replay Results"),
    ] {
        test_chart(history, &assets.join(format!("{name}.svg")), kind, title)?;
    }
    let mut section="## Tests, coverage and benchmarks\n\nGenerated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.\n\n### Cargo tests\n\n".to_owned();
    let cargo = latest(history, "cargo");
    let full = latest(history, "full");
    if let Some(r) = cargo {
        section.push_str(&format!("Latest Cargo run: {}\n\n", run_text(r)?));
    } else {
        section.push_str("Awaiting the first separate Cargo test run. Earlier combined runs remain in the history.\n\n");
    }
    section.push_str("![Cargo test results](docs/reports/cargo-history.svg)\n\n");
    if let Some(t) = cargo.map(|r| &r["data"]["tests"]).filter(|t| !t.is_null()) {
        section.push_str(&format!("| Total | Passed | Failed | Errors | Skipped |\n| ---: | ---: | ---: | ---: | ---: |\n| {} | {} | [{}](docs/reports/failed-tests.md) | {} | {} |\n\n",t["total"],t["passed"],t["failed"],t["errors"],t["skipped"]));
    }
    section.push_str("[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).\n\n");
    for (kind, title) in [
        ("models", "TLA+ models and Rust trace replays"),
        ("fuzz", "Fuzzing: libFuzzer and AFL++"),
    ] {
        section.push_str(&format!("### {title}\n\n"));
        if let Some(r) = latest(history, kind) {
            section.push_str(&format!("{}\n\n", run_text(r)?));
        } else {
            section.push_str(
                "Awaiting the first dedicated CI run; no measurements have been invented.\n\n",
            );
        }
        if kind == "models" {
            section.push_str("![TLA+ Rust replay results](docs/reports/tla-history.svg)\n\n");
        }
        if let Some(r) = latest(history, kind) {
            section.push_str(&charts_text(r, &assets)?);
        }
        section.push_str(if kind=="models"{"[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.\n\n"}else{"AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.\n\n"});
    }
    section.push_str("### LLVM coverage\n\n");
    if let Some(r) = cargo
        .or(full)
        .filter(|r| !r["data"]["charts"].as_array().unwrap().is_empty())
    {
        section.push_str(&charts_text(r, &assets)?);
    } else {
        section.push_str("No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.\n\n");
    }
    section.push_str("### Linux loopback benchmark comparisons\n\n");
    if let Some(r) = latest(history, "benchmark") {
        section.push_str(&format!("Latest benchmark run: {}\n\n", run_text(r)?));
    } else {
        section.push_str("Awaiting the first dedicated DigitalOcean benchmark run.\n\n");
    }
    section.push_str("Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Capntproto uses encrypted Native/UDP; C++ Cap'n Proto, gRPC and WebSocket baselines use plaintext TCP. Bars compare this workload, not universal protocol performance.\n\n");
    if let Some(r) =
        latest(history, "benchmark").filter(|r| !r["data"]["charts"].as_array().unwrap().is_empty())
    {
        section.push_str(&charts_text(r, &assets)?);
    } else {
        let mut svg = start("Capntproto — Linux Loopback Benchmarks", 240);
        text(
            &mut svg,
            35.0,
            65.0,
            27,
            "Capntproto — Linux Loopback Benchmarks",
            "#1e293b",
        );
        text(
            &mut svg,
            35.0,
            130.0,
            21,
            "Awaiting a validated dedicated-host benchmark run",
            "#64748b",
        );
        text(
            &mut svg,
            35.0,
            190.0,
            15,
            "Capntproto / Native · C++ Cap'n Proto · gRPC (tonic) · WebSockets",
            "#334155",
        );
        finish(svg, &assets.join("benchmarks-pending.svg"))?;
        section
            .push_str("![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)\n\n");
    }
    section.push_str("[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.\n");
    crate::write(
        output.join("README.md"),
        template.replace("{{REPORTS}}", &section),
    )?;
    crate::write(
        assets.join("failed-tests.md"),
        failure_report(cargo.or(full))?,
    )?;
    crate::write(
        assets.join("failed-models.md"),
        failure_report(latest(history, "models"))?.replacen(
            "# Failed workspace tests",
            "# Failed TLA+ replay tests",
            1,
        ),
    )?;
    crate::write_json(assets.join("history.json"), history)?;
    Ok(owned()
        .into_iter()
        .map(|p| output.join(p))
        .filter(|p| p.is_file())
        .collect())
}
