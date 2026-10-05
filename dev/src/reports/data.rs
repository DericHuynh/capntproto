use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path};
pub const CHARTS: [&str; 16] = [
    "latency-p50",
    "latency-p95",
    "latency-p99",
    "request-rate",
    "latency-difference",
    "request-rate-difference",
    "instruction-counts",
    "fuzz-executions",
    "fuzz-coverage",
    "fuzz-findings",
    "tla-outcomes",
    "tla-states",
    "coverage-lines",
    "coverage-regions",
    "coverage-functions",
    "coverage-branches",
];
const COUNTS: [&str; 4] = ["passed", "failed", "errors", "skipped"];
pub fn integer(value: &Value) -> Result<u64> {
    let n = value.as_u64().context("invalid test counter")?;
    ensure!(n <= 10_000_000, "invalid test counter");
    Ok(n)
}
pub fn matches(pattern: &str, text: &str) -> bool {
    static PATTERNS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::BTreeMap<String, regex::Regex>>,
    > = std::sync::OnceLock::new();
    let regex = PATTERNS
        .get_or_init(Default::default)
        .lock()
        .expect("pattern cache")
        .entry(pattern.to_owned())
        .or_insert_with(|| regex::Regex::new(pattern).expect("constant expression"))
        .clone();
    regex.is_match(text)
}
fn keys(value: &Value, names: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.len() == names.len() && names.iter().all(|n| o.contains_key(*n)))
}
fn bounded(value: &Value, limit: usize) -> bool {
    value.as_str().is_some_and(|s| s.chars().count() <= limit)
}
fn flag(value: &Value) -> Result<bool> {
    value.as_bool().context("invalid boolean")
}
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    pub passed: u64,
    pub failed: u64,
    pub errors: u64,
    pub skipped: u64,
    pub total: u64,
    pub suites: u64,
    pub complete: bool,
    pub command_passed: bool,
    pub failures: Vec<Failure>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub suite: String,
    pub name: String,
    pub output: String,
    pub truncated: bool,
}
fn excerpt(suite: &str, name: &str, output: &str, budget: &mut usize) -> Failure {
    let output = regex::Regex::new(r"\x1b\[[0-9;]*[A-Za-z]")
        .unwrap()
        .replace_all(output, "");
    let limit = 4096.min(*budget);
    let text = output.chars().take(limit).collect::<String>();
    let len = text.chars().count();
    *budget -= len;
    Failure {
        suite: suite.into(),
        name: name.into(),
        truncated: output.chars().count() > len,
        output: text,
    }
}
impl Counts {
    fn count(&self) -> u64 {
        self.passed + self.failed + self.errors + self.skipped
    }
    fn append(&mut self, other: &Counts) {
        self.passed += other.passed;
        self.failed += other.failed;
        self.errors += other.errors;
        self.skipped += other.skipped;
        self.suites += other.suites;
        self.failures.extend(other.failures.clone());
        self.total = self.count();
    }
}
struct Active {
    announced: u64,
    counts: Counts,
    names: BTreeSet<String>,
    label: String,
}
fn close(
    active: &mut Option<Active>,
    total: &mut Counts,
    incomplete: bool,
    unfinished: &mut bool,
) -> Result<()> {
    if let Some(mut a) = active.take() {
        ensure!(
            a.counts.count() <= a.announced,
            "libtest counters disagree with announced tests"
        );
        let remaining = a.announced - a.counts.count();
        ensure!(
            incomplete || remaining == 0,
            "libtest counters disagree with announced tests"
        );
        a.counts.errors += remaining;
        a.counts.suites = 1;
        total.append(&a.counts);
        *unfinished |= incomplete;
    }
    Ok(())
}
pub fn test_counts(log: &str, passed: bool, allow_filtered: bool) -> Result<Option<Counts>> {
    let mut totals = Counts::default();
    let mut active: Option<Active> = None;
    let mut unfinished = false;
    let mut label = String::new();
    let mut budget = 256 * 1024;
    for line in log.lines() {
        if !line.starts_with('{') {
            let s = line.trim_start();
            if s.starts_with("Running ") || s.starts_with("Doc-tests ") {
                label = s.chars().take(1024).collect();
            }
            continue;
        }
        let Ok(item) = serde_json::from_str::<Value>(line) else {
            if active.is_some() {
                unfinished = true;
            }
            continue;
        };
        let kind = item["type"].as_str().unwrap_or("");
        let event = item["event"].as_str().unwrap_or("");
        match (kind, event) {
            ("suite", "started") => {
                close(&mut active, &mut totals, true, &mut unfinished)?;
                active = Some(Active {
                    announced: integer(&item["test_count"])?,
                    counts: Counts::default(),
                    names: BTreeSet::new(),
                    label: if label.is_empty() {
                        format!("Suite {}", totals.suites + 1)
                    } else {
                        label.clone()
                    },
                });
            }
            ("test", "ok" | "failed" | "ignored") => {
                let a = active.as_mut().context("test result outside a suite")?;
                let name = crate::string(&item, "name")?;
                ensure!(a.names.insert(name.into()), "duplicate test result");
                match event {
                    "ok" => a.counts.passed += 1,
                    "ignored" => a.counts.skipped += 1,
                    _ => {
                        a.counts.failed += 1;
                        let output = item
                            .get("stdout")
                            .map(|v| v.as_str().context("invalid failed-test output"))
                            .transpose()?
                            .unwrap_or("");
                        a.counts
                            .failures
                            .push(excerpt(&a.label, name, output, &mut budget));
                    }
                }
            }
            ("suite", "ok" | "failed") => {
                let a = active.as_ref().context("suite result without start")?;
                ensure!(
                    integer(&item["passed"])? == a.counts.passed
                        && integer(&item["failed"])? == a.counts.failed
                        && integer(&item["ignored"])? == a.counts.skipped,
                    "libtest summary disagrees with test events"
                );
                ensure!(
                    integer(item.get("measured").unwrap_or(&json!(0)))? == 0
                        && (allow_filtered
                            || integer(item.get("filtered_out").unwrap_or(&json!(0)))? == 0),
                    "filtered/benchmark-only execution is not a full workspace run"
                );
                close(&mut active, &mut totals, false, &mut unfinished)?;
            }
            _ => {}
        }
    }
    close(&mut active, &mut totals, true, &mut unfinished)?;
    if totals.suites == 0 {
        ensure!(
            !passed,
            "successful workspace command produced no test events"
        );
        return Ok(None);
    }
    ensure!(
        !passed || (!unfinished && totals.failed == 0 && totals.errors == 0),
        "successful command has failed/incomplete tests"
    );
    totals.complete = passed && !unfinished;
    totals.command_passed = passed;
    Ok(Some(totals))
}
fn validate_xml_counts(node: roxmltree::Node<'_, '_>, counts: &Counts) -> Result<()> {
    for (attr, expected) in [
        ("tests", counts.count()),
        ("failures", counts.failed),
        ("errors", counts.errors),
        ("skipped", counts.skipped),
    ] {
        let value = node.attribute(attr).context("missing JUnit counter")?;
        ensure!(
            !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()),
            "invalid JUnit counter"
        );
        let actual = value.parse::<u64>()?;
        ensure!(
            actual <= 10_000_000 && actual == expected,
            "JUnit counters disagree with test cases"
        );
    }
    Ok(())
}
pub fn junit_counts(xml: &[u8], passed: bool) -> Result<Counts> {
    ensure!(xml.len() <= 64 * 1024 * 1024, "oversized JUnit report");
    let text = std::str::from_utf8(xml)?;
    ensure!(
        !text.contains('\0') && !text.contains("<!DOCTYPE") && !text.contains("<!ENTITY"),
        "unsafe JUnit report"
    );
    let document = roxmltree::Document::parse(text)?;
    let root = document.root_element();
    ensure!(
        root.has_tag_name("testsuites"),
        "expected nextest testsuites"
    );
    let mut totals = Counts::default();
    let mut identities = BTreeSet::new();
    let mut budget = 256 * 1024;
    for suite in root.children().filter(|n| n.has_tag_name("testsuite")) {
        let mut counts = Counts {
            suites: 1,
            ..Default::default()
        };
        for case in suite.children().filter(|n| n.has_tag_name("testcase")) {
            let name = case
                .attribute("name")
                .filter(|s| !s.is_empty())
                .context("missing JUnit test name")?;
            let label = case
                .attribute("classname")
                .or_else(|| suite.attribute("name"))
                .filter(|s| !s.is_empty())
                .context("missing JUnit test suite")?;
            ensure!(
                identities.insert((label, name)),
                "duplicate JUnit test identity"
            );
            let outcomes: Vec<_> = case
                .children()
                .filter(|n| matches!(n.tag_name().name(), "failure" | "error" | "skipped"))
                .collect();
            ensure!(outcomes.len() <= 1, "conflicting JUnit outcomes");
            let outcome = outcomes
                .first()
                .map(|n| n.tag_name().name())
                .unwrap_or("passed");
            match outcome {
                "failure" => counts.failed += 1,
                "error" => counts.errors += 1,
                "skipped" => counts.skipped += 1,
                _ => counts.passed += 1,
            };
            if matches!(outcome, "failure" | "error") {
                let output = case
                    .children()
                    .filter(|n| {
                        matches!(
                            n.tag_name().name(),
                            "failure" | "error" | "system-out" | "system-err"
                        )
                    })
                    .map(|n| {
                        n.descendants()
                            .filter(|n| n.is_text())
                            .filter_map(|n| n.text())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                counts
                    .failures
                    .push(excerpt(label, name, &output, &mut budget));
            }
        }
        validate_xml_counts(suite, &counts)?;
        totals.append(&counts);
    }
    validate_xml_counts(root, &totals)?;
    ensure!(
        !passed || (totals.suites > 0 && totals.failed == 0 && totals.errors == 0),
        "successful nextest command has missing/failed tests"
    );
    totals.complete = passed;
    totals.command_passed = passed;
    Ok(totals)
}
fn merge_tests(native: Option<Counts>, docs: Option<Counts>) -> Option<Counts> {
    match (native, docs) {
        (None, None) => None,
        (Some(mut c), None) | (None, Some(mut c)) => {
            c.complete = false;
            c.command_passed = false;
            Some(c)
        }
        (Some(mut n), Some(d)) => {
            n.append(&d);
            n.complete &= d.complete;
            n.command_passed &= d.command_passed;
            let mut budget = 256 * 1024;
            for failure in &mut n.failures {
                let clipped = excerpt(&failure.suite, &failure.name, &failure.output, &mut budget);
                failure.truncated |= clipped.truncated;
                failure.output = clipped.output;
            }
            Some(n)
        }
    }
}
pub fn chart_kind(name: &str, kind: &str) -> bool {
    match kind {
        "full" | "cargo" => name.starts_with("coverage-"),
        "models" => name.starts_with("tla-"),
        "fuzz" => name.starts_with("fuzz-"),
        _ => !["coverage-", "tla-", "fuzz-"]
            .iter()
            .any(|p| name.starts_with(p)),
    }
}
pub fn validate_charts(value: &Value) -> Result<()> {
    let charts = value.as_array().context("invalid chart collection")?;
    ensure!(charts.len() <= CHARTS.len(), "invalid chart collection");
    let mut names = BTreeSet::new();
    for chart in charts {
        let name = crate::string(chart, "name")?;
        ensure!(
            CHARTS.contains(&name) && names.insert(name),
            "unexpected or duplicate chart"
        );
        for field in ["title", "unit", "note"] {
            ensure!(bounded(&chart[field], 500), "invalid chart label");
        }
        let panels = chart["panels"].as_array().context("invalid panels")?;
        ensure!((1..=4).contains(&panels.len()), "invalid panel count");
        for panel in panels {
            ensure!(bounded(&panel["title"], 150), "invalid panel title");
            let bars = panel["bars"].as_array().context("invalid bars")?;
            ensure!((1..=40).contains(&bars.len()), "invalid bar count");
            for bar in bars {
                ensure!(bounded(&bar["label"], 150), "invalid bar label");
                let value = &bar["value"];
                if !value.is_null() {
                    ensure!(
                        value
                            .as_f64()
                            .is_some_and(|v| v.is_finite() && v.abs() <= 1e20),
                        "invalid bar value"
                    );
                    if name.starts_with("fuzz-") || name.starts_with("tla-") {
                        ensure!(
                            value.as_u64().is_some(),
                            "verification counters must be nonnegative integers"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
pub fn validate_publication(value: &Value) -> Result<()> {
    ensure!(
        keys(
            value,
            &[
                "format",
                "kind",
                "origin",
                "source_id",
                "tests",
                "charts",
                "status",
                "note"
            ]
        ),
        "unexpected publication fields"
    );
    let kind = crate::string(value, "kind")?;
    ensure!(
        value["format"] == 1 && ["full", "cargo", "models", "fuzz", "benchmark"].contains(&kind),
        "invalid publication format/kind"
    );
    ensure!(
        ["passed", "failed", "unavailable"].contains(&crate::string(value, "status")?),
        "invalid publication status"
    );
    ensure!(bounded(&value["note"], 500), "invalid publication note");
    let origin = &value["origin"];
    if !origin.is_null() {
        ensure!(
            keys(origin, &["repository", "run_id", "attempt", "commit"])
                && matches(r"^[\w.-]+/[\w.-]+$", crate::string(origin, "repository")?)
                && crate::number(origin, "run_id")? > 0
                && crate::number(origin, "attempt")? > 0
                && matches(r"^[a-f0-9]{40}$", crate::string(origin, "commit")?),
            "invalid publication origin"
        );
    }
    let source = &value["source_id"];
    ensure!(
        source.is_null()
            || source
                .as_str()
                .is_some_and(|s| matches(r"^[a-f0-9]{64}$", s)),
        "invalid source fingerprint"
    );
    let tests = &value["tests"];
    if !tests.is_null() {
        ensure!(
            ["full", "cargo", "models"].contains(&kind),
            "invalid test summary lane"
        );
        let required = [
            "passed",
            "failed",
            "errors",
            "skipped",
            "total",
            "suites",
            "complete",
            "command_passed",
        ];
        let object = tests.as_object().context("invalid test summary")?;
        ensure!(
            required.iter().all(|k| object.contains_key(*k))
                && object
                    .keys()
                    .all(|k| required.contains(&k.as_str()) || k == "failures"),
            "invalid test summary fields"
        );
        let mut total = 0;
        for key in COUNTS {
            total += integer(&tests[key])?;
        }
        ensure!(
            integer(&tests["total"])? == total,
            "invalid aggregate test total"
        );
        integer(&tests["suites"])?;
        let complete = flag(&tests["complete"])?;
        let command = flag(&tests["command_passed"])?;
        ensure!(
            !complete || (command && tests["failed"] == 0 && tests["errors"] == 0),
            "invalid successful test summary"
        );
        if let Some(failures) = tests.get("failures") {
            let failures = failures
                .as_array()
                .context("invalid failed-test inventory")?;
            let failed = integer(&tests["failed"])?;
            let errors = integer(&tests["errors"])?;
            ensure!(
                failures.len() <= 4096
                    && (failed..=failed + errors).contains(&(failures.len() as u64)),
                "failed-test inventory disagrees with count or exceeds limit"
            );
            let mut output = 0;
            for f in failures {
                ensure!(
                    keys(f, &["suite", "name", "output", "truncated"])
                        && bounded(&f["suite"], 1024)
                        && bounded(&f["name"], 1024)
                        && bounded(&f["output"], 4096)
                        && !crate::string(f, "name")?.is_empty()
                        && f["truncated"].is_boolean(),
                    "invalid failed-test details"
                );
                output += crate::string(f, "output")?.chars().count();
            }
            ensure!(output <= 256 * 1024, "failed-test output exceeds budget");
        }
    }
    validate_charts(&value["charts"])?;
    let charts = value["charts"].as_array().unwrap();
    ensure!(
        charts
            .iter()
            .all(|c| chart_kind(c["name"].as_str().unwrap_or(""), kind)),
        "chart lane mismatch"
    );
    ensure!(
        (tests.is_null() && value["status"] != "passed") || !source.is_null(),
        "measured results require a source fingerprint"
    );
    ensure!(
        charts.is_empty()
            || (!source.is_null()
                && (value["status"] == "passed" || ["models", "fuzz"].contains(&kind))),
        "charts require successful source-bound evidence"
    );
    Ok(())
}
pub fn unavailable(kind: &str, note: &str) -> Value {
    json!({"format":1,"kind":kind,"origin":null,"source_id":null,"tests":null,"charts":[],"status":"unavailable","note":note})
}
pub fn collect(directory: &Path, kind: &str) -> Result<Value> {
    let lane = match kind {
        "full" | "cargo" => "coverage",
        "models" => "models",
        "fuzz" => "fuzz",
        "benchmark" => "benchmark",
        _ => bail!("invalid lane"),
    };
    let mut result = unavailable(kind, "No measurement evidence was produced.");
    if let Ok(run) = std::env::var("GITHUB_RUN_ID") {
        result["origin"] = json!({"repository":std::env::var("GITHUB_REPOSITORY")?,"run_id":run.parse::<u64>()?,"attempt":std::env::var("GITHUB_RUN_ATTEMPT")?.parse::<u64>()?,"commit":std::env::var("GITHUB_SHA")?});
    }
    let evidence_path = directory.join(lane).join("evidence.json");
    if !evidence_path.exists() {
        validate_publication(&result)?;
        return Ok(result);
    }
    let evidence = crate::read_json(&evidence_path)?;
    ensure!(
        evidence["format"] == 1 && evidence["lane"] == lane,
        "unexpected evidence format/lane"
    );
    let source = crate::string(&evidence, "source_id")?;
    ensure!(
        matches(r"^[a-f0-9]{64}$", source),
        "invalid source fingerprint"
    );
    let passed = flag(&evidence["passed"])?;
    result["source_id"] = json!(source);
    result["status"] = json!(if passed { "passed" } else { "failed" });
    result["note"] = json!(if passed {
        "Evidence passed."
    } else {
        "Lane failed or did not complete; inspect the linked CI run."
    });
    let mut nextest = false;
    let mut tests = None;
    let mut docs = None;
    for step in evidence["steps"]
        .as_array()
        .context("missing evidence steps")?
    {
        let name = crate::string(step, "name")?;
        ensure!(matches(r"^[a-zA-Z0-9-]+$", name), "unsafe log name");
        let log = std::fs::read(
            directory
                .join(lane)
                .join("logs")
                .join(format!("{name}.log")),
        )?;
        ensure!(
            json!(crate::hash(&log)) == step["log_sha256"],
            "evidence log hash mismatch"
        );
        let step_passed = flag(&step["passed"])?;
        if (["full", "cargo"].contains(&kind) && name == "workspace-tests")
            || (kind == "models" && name == "model-tests")
        {
            let command: Vec<String> = serde_json::from_value(step["command"].clone())?;
            let has = |s: &str| command.iter().any(|v| v == s);
            ensure!(
                has("--workspace") && has("--no-fail-fast"),
                "test evidence is not the complete workspace invocation"
            );
            ensure!(
                kind != "models" || has("tlc"),
                "model partition must select TLC tests"
            );
            if has("nextest") {
                nextest = true;
                ensure!(
                    kind != "cargo" || has("-E"),
                    "Cargo partition must exclude dedicated checks"
                );
                let report = &evidence["data"]["test_reports"][name];
                if report.is_null() {
                    ensure!(!step_passed, "successful nextest run is missing JUnit");
                    tests = None;
                } else {
                    ensure!(
                        report["file"] == format!("{name}.xml"),
                        "unexpected JUnit filename"
                    );
                    let xml = std::fs::read(
                        directory
                            .join(lane)
                            .join("logs")
                            .join(crate::string(report, "file")?),
                    )?;
                    ensure!(
                        json!(crate::hash(&xml)) == report["sha256"],
                        "JUnit hash mismatch"
                    );
                    tests = Some(junit_counts(&xml, step_passed)?);
                }
            } else {
                ensure!(
                    has("--format=json") && (kind != "cargo" || has("--skip")),
                    "invalid historical libtest invocation"
                );
                tests = test_counts(std::str::from_utf8(&log)?, step_passed, kind != "full")?;
            }
        } else if name == "workspace-doctests" && ["full", "cargo"].contains(&kind) {
            let command: Vec<String> = serde_json::from_value(step["command"].clone())?;
            ensure!(
                ["--workspace", "--doc", "--format=json"]
                    .iter()
                    .all(|s| command.iter().any(|v| v == s)),
                "incomplete doctest invocation"
            );
            docs = test_counts(std::str::from_utf8(&log)?, step_passed, false)?;
        }
    }
    if nextest && ["full", "cargo"].contains(&kind) {
        ensure!(
            !passed || docs.is_some(),
            "successful coverage lane is missing doctests"
        );
        tests = merge_tests(tests, docs);
    }
    result["tests"] = serde_json::to_value(tests)?;
    let path = directory.join("charts/data.json");
    if path.exists() {
        let data = crate::read_json(&path)?;
        ensure!(
            data["source_id"] == source && passed,
            "stale/failed chart evidence"
        );
        validate_charts(&data["charts"])?;
        ensure!(
            data["charts"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| chart_kind(c["name"].as_str().unwrap_or(""), kind)),
            "chart lane mismatch"
        );
        result["charts"] = data["charts"].clone();
    }
    if kind == "models" {
        result["charts"] = json!(super::verification::models(
            &evidence["data"],
            &directory.join(lane)
        )?);
    } else if kind == "fuzz" {
        result["charts"] = json!(super::verification::fuzz(&evidence["data"])?);
    }
    validate_publication(&result)?;
    Ok(result)
}
