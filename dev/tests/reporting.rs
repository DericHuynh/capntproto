use capntproto_dev::{
    hash,
    reports::{data, render},
    write, write_json,
};
use serde_json::{json, Value};
fn suite(passed: usize, failed: usize, ignored: usize) -> String {
    let mut rows =
        vec![json!({"type":"suite","event":"started","test_count":passed+failed+ignored})];
    for (event, n) in [("ok", passed), ("failed", failed), ("ignored", ignored)] {
        for i in 0..n {
            rows.push(json!({"type":"test","event":event,"name":format!("{event}_{i}"),"stdout":"test result: ok. 99 passed;\n{\"type\":\"suite\"}"}));
        }
    }
    rows.push(json!({"type":"suite","event":if failed>0{"failed"}else{"ok"},"passed":passed,"failed":failed,"ignored":ignored,"measured":0,"filtered_out":0}));
    rows.iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}
fn publication(kind: &str) -> Value {
    json!({"format":1,"kind":kind,"origin":null,"source_id":"a".repeat(64),"status":"passed","note":"Synthetic unit-test fixture, not a measurement.","tests":if kind=="full"{serde_json::to_value(data::test_counts(&suite(3,0,1),true,false).unwrap()).unwrap()}else{Value::Null},"charts":[]})
}
fn record(id: u64, kind: &str, date: &str, attempt: u64) -> Value {
    json!({"run_id":id,"attempt":attempt,"date":date,"commit":"b".repeat(40),"url":format!("https://github.com/example/capntproto/actions/runs/{id}"),"conclusion":"success","data":publication(kind)})
}
const DATE: &str = "2026-09-28T10:00:00Z";
const XML: &str = r#"<testsuites tests="4" failures="1" errors="1" skipped="1"><testsuite name="crate" tests="4" failures="1" errors="1" skipped="1"><testcase name="ok" classname="crate"/><testcase name="ignored" classname="crate"><skipped/></testcase><testcase name="bad" classname="crate"><failure>assertion failed</failure></testcase><testcase name="abort" classname="crate"><error>signal 6</error></testcase></testsuite></testsuites>"#;
#[test]
fn counts_ignore_nested_test_output() {
    let counts = data::test_counts(
        &format!("cargo output\n{}\n{}", suite(2, 1, 1), suite(5, 0, 0)),
        false,
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        [
            counts.total,
            counts.passed,
            counts.failed,
            counts.skipped,
            counts.errors,
            counts.suites
        ],
        [9, 7, 1, 1, 0, 2]
    );
    assert!(!counts.complete);
}
#[test]
fn failures_keep_suite_identity_and_escape_diagnostics() {
    let output = format!(
        "    Running tests/a.rs (target/a)\n{}\n   Doc-tests example\n{}",
        suite(0, 1, 0),
        suite(0, 1, 0)
    );
    let mut counts = data::test_counts(&output, false, false).unwrap().unwrap();
    assert_eq!(counts.failures.len(), 2);
    assert_ne!(counts.failures[0].suite, counts.failures[1].suite);
    counts.failures[0].output = "<script>alert(1)</script> [click](javascript:x)".into();
    let mut p = publication("full");
    p["tests"] = json!(counts);
    let report = render::failure_details(&p).unwrap();
    assert_eq!(report.matches("<details open>").count(), 2);
    assert!(report.contains("&lt;script&gt;") && !report.contains("<script>"));
    assert!(report.contains("**2 failed**"));
}
#[test]
fn failure_diagnostics_are_bounded() {
    let counts = data::test_counts(
        &suite(0, 80, 0).replace("test result: ok. 99 passed;", &"x".repeat(5000)),
        false,
        false,
    )
    .unwrap()
    .unwrap();
    assert!(counts.failures.iter().all(|f| f.truncated));
    assert!(
        counts
            .failures
            .iter()
            .map(|f| f.output.len())
            .sum::<usize>()
            <= 256 * 1024
    );
    let mut p = publication("full");
    p["tests"] = json!(counts);
    data::validate_publication(&p).unwrap();
    p["tests"]["failures"].as_array_mut().unwrap().pop();
    assert!(data::validate_publication(&p).is_err());
    p["tests"].as_object_mut().unwrap().remove("failures");
    data::validate_publication(&p).unwrap();
}
#[test]
fn abort_and_build_failure_are_distinct() {
    assert!(data::test_counts("error: could not compile", false, false)
        .unwrap()
        .is_none());
    let partial="{\"type\":\"suite\",\"event\":\"started\",\"test_count\":4}\n{\"type\":\"test\",\"event\":\"ok\",\"name\":\"finished\"}";
    let counts = data::test_counts(partial, false, false).unwrap().unwrap();
    assert_eq!([counts.total, counts.passed, counts.errors], [4, 1, 3]);
    assert!(data::test_counts(partial, true, false).is_err());
    let counts = data::test_counts(&format!("{partial}\n{}", suite(2, 0, 0)), false, false)
        .unwrap()
        .unwrap();
    assert_eq!([counts.total, counts.passed, counts.errors], [6, 3, 3]);
}
#[test]
fn duplicate_filtered_and_inconsistent_counts_fail() {
    let source = suite(2, 1, 1);
    for bad in [
        source.replace("\"passed\":2", "\"passed\":3"),
        source.replace("\"filtered_out\":0", "\"filtered_out\":8"),
        source.replace("ok_1", "ok_0"),
    ] {
        assert!(data::test_counts(&bad, false, false).is_err());
    }
    let filtered = suite(3, 0, 1).replace("\"filtered_out\":0", "\"filtered_out\":99");
    assert_eq!(
        data::test_counts(&filtered, true, true)
            .unwrap()
            .unwrap()
            .total,
        4
    );
}
#[test]
fn nextest_failed_and_aborted_tests_keep_diagnostics() {
    let counts = data::junit_counts(XML.as_bytes(), false).unwrap();
    assert_eq!(
        [
            counts.total,
            counts.passed,
            counts.failed,
            counts.errors,
            counts.skipped
        ],
        [4, 1, 1, 1, 1]
    );
    assert_eq!(
        counts
            .failures
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["bad", "abort"]
    );
    assert!(data::junit_counts(XML.as_bytes(), true).is_err());
}
#[test]
fn corrupt_duplicate_or_unsafe_xml_fails() {
    for xml in [
        XML.as_bytes()[..XML.len() - 5].to_vec(),
        XML.replace("tests=\"4\"", "tests=\"5\"").into_bytes(),
        XML.replace("name=\"abort\"", "name=\"bad\"").into_bytes(),
        format!("<!DOCTYPE testsuites [<!ENTITY x \"expand\">]>{XML}").into_bytes(),
        XML.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        XML.replace(
            "<error>signal 6</error>",
            "<error>signal 6</error><failure/>",
        )
        .into_bytes(),
    ] {
        assert!(data::junit_counts(&xml, false).is_err());
    }
}
#[test]
fn collector_binds_logs_and_junit_and_requires_doctests() {
    let dir = tempfile::tempdir().unwrap();
    let lane = dir.path().join("coverage");
    write(lane.join("logs/workspace-tests.log"), b"nextest failed").unwrap();
    write(lane.join("logs/workspace-tests.xml"), XML).unwrap();
    let mut evidence = json!({"format":1,"lane":"coverage","source_id":"a".repeat(64),"passed":false,"steps":[{"name":"workspace-tests","command":["cargo","nextest","run","--workspace","--no-fail-fast","-E","not test(tlc)"],"passed":false,"log_sha256":hash(b"nextest failed")}],"data":{"test_reports":{"workspace-tests":{"file":"workspace-tests.xml","sha256":hash(XML)}}}});
    write_json(lane.join("evidence.json"), &evidence).unwrap();
    let result = data::collect(dir.path(), "cargo").unwrap();
    assert_eq!(result["tests"]["total"], 4);
    assert_eq!(result["tests"]["complete"], false);
    write(lane.join("logs/workspace-tests.xml"), format!("{XML} ")).unwrap();
    assert!(data::collect(dir.path(), "cargo")
        .unwrap_err()
        .to_string()
        .contains("JUnit hash mismatch"));
    evidence["data"]["test_reports"]
        .as_object_mut()
        .unwrap()
        .remove("workspace-tests");
    evidence["steps"][0]["passed"] = json!(true);
    write_json(lane.join("evidence.json"), &evidence).unwrap();
    assert!(data::collect(dir.path(), "cargo")
        .unwrap_err()
        .to_string()
        .contains("missing JUnit"));
    let clean = XML
        .replace("failures=\"1\"", "failures=\"0\"")
        .replace("errors=\"1\"", "errors=\"0\"")
        .replace("<failure>assertion failed</failure>", "")
        .replace("<error>signal 6</error>", "");
    write(lane.join("logs/workspace-tests.xml"), &clean).unwrap();
    evidence["data"]["test_reports"]["workspace-tests"] =
        json!({"file":"workspace-tests.xml","sha256":hash(&clean)});
    evidence["passed"] = json!(true);
    write_json(lane.join("evidence.json"), &evidence).unwrap();
    assert!(data::collect(dir.path(), "cargo")
        .unwrap_err()
        .to_string()
        .contains("missing doctests"));
    let docs = suite(5, 0, 1);
    write(lane.join("logs/workspace-doctests.log"), &docs).unwrap();
    evidence["steps"].as_array_mut().unwrap().push(json!({"name":"workspace-doctests","command":["cargo","test","--workspace","--doc","--format=json"],"passed":true,"log_sha256":hash(&docs)}));
    write_json(lane.join("evidence.json"), &evidence).unwrap();
    let result = data::collect(dir.path(), "cargo").unwrap();
    assert_eq!(result["tests"]["total"], 10);
    assert_eq!(result["tests"]["complete"], true);
    write(lane.join("logs/workspace-doctests.log"), "modified").unwrap();
    assert!(data::collect(dir.path(), "cargo").is_err());
}
#[test]
fn chart_names_and_lanes_are_validated() {
    let mut p = publication("benchmark");
    p["charts"] = json!([{"name":"../README","title":"x","unit":"x","note":"","panels":[]}]);
    assert!(data::validate_publication(&p).is_err());
    p["charts"][0] = json!({"name":"fuzz-executions","title":"x","unit":"x","note":"","panels":[{"title":"x","bars":[{"label":"x","value":10}]}]});
    assert!(data::validate_publication(&p).is_err());
    p["kind"] = json!("fuzz");
    data::validate_publication(&p).unwrap();
    p["charts"][0]["panels"][0]["bars"][0]["value"] = json!(-1);
    assert!(data::validate_publication(&p).is_err());
}
#[test]
fn history_orders_deduplicates_and_replaces_reruns() {
    let history = render::merge(
        &render::empty_history(),
        &record(2, "full", "2026-09-29T10:00:00Z", 1),
    )
    .unwrap();
    let history = render::merge(&history, &record(1, "full", DATE, 1)).unwrap();
    assert_eq!(history["full"][0]["run_id"], 1);
    assert_eq!(
        render::merge(&history, &record(1, "full", DATE, 1)).unwrap(),
        history
    );
    let mut rerun = record(1, "full", DATE, 2);
    rerun["conclusion"] = json!("failure");
    rerun["data"]["status"] = json!("failed");
    rerun["data"]["tests"] = Value::Null;
    let history = render::merge(&history, &rerun).unwrap();
    assert_eq!(history["full"][0]["attempt"], 2);
    assert!(history["full"][0]["data"]["tests"].is_null());
    let history = render::merge(&history, &record(3, "benchmark", DATE, 1)).unwrap();
    let mut failed = record(4, "benchmark", "2026-09-29T10:00:00Z", 1);
    failed["data"]["status"] = json!("failed");
    let history = render::merge(&history, &failed).unwrap();
    assert_eq!(history["benchmark"]["run_id"], 4);
}
#[test]
fn history_rejects_injected_links_dates_and_counts() {
    for (k, v) in [
        ("url", "javascript:alert(1)"),
        ("commit", "../README"),
        ("date", "today"),
    ] {
        let mut r = record(1, "full", DATE, 1);
        r[k] = json!(v);
        assert!(render::merge(&render::empty_history(), &r).is_err());
    }
    let mut r = record(1, "full", DATE, 1);
    r["data"]["tests"]["total"] = json!(500);
    assert!(render::merge(&render::empty_history(), &r).is_err());
}
#[test]
fn history_is_bounded_and_partitions_remain_separate() {
    let mut history = render::empty_history();
    history["full"] = json!((1..=365)
        .map(|id| record(id, "full", DATE, 1))
        .collect::<Vec<_>>());
    for id in 366..371 {
        history = render::merge(&history, &record(id, "full", DATE, 1)).unwrap();
    }
    assert_eq!(history["full"].as_array().unwrap().len(), 365);
    assert!(history["full"][0]["data"]["tests"]
        .get("failures")
        .is_none());
    assert!(history["full"][364]["data"]["tests"]
        .get("failures")
        .is_some());
    for (i, lane) in ["cargo", "models", "fuzz"].into_iter().enumerate() {
        history = render::merge(&history, &record(400 + i as u64, lane, DATE, 1)).unwrap();
        assert_eq!(history[lane].as_array().unwrap().len(), 1);
    }
    render::validate_history(&history).unwrap();
}

#[test]
fn shared_ci_run_ids_require_consistent_sources_and_unique_lanes() {
    let history = render::merge(&render::empty_history(), &record(1, "cargo", DATE, 1)).unwrap();
    let history = render::merge(&history, &record(1, "models", DATE, 1)).unwrap();
    let mut corrupt = history.clone();
    corrupt["models"][0]["commit"] = json!("c".repeat(40));
    assert!(render::validate_history(&corrupt).is_err());
    let mut duplicate = history.clone();
    duplicate["cargo"]
        .as_array_mut()
        .unwrap()
        .push(history["cargo"][0].clone());
    assert!(render::validate_history(&duplicate).is_err());
    render::validate_history(&history).unwrap();
}
#[test]
fn renderer_has_labels_is_deterministic_and_removes_stale_charts() {
    let dir = tempfile::tempdir().unwrap();
    let template = "# Synthetic renderer fixture — not measured results\n\n{{REPORTS}}";
    let files = render::render(template, &render::empty_history(), dir.path()).unwrap();
    assert!(files
        .iter()
        .all(|f| render::owned().contains(f.strip_prefix(dir.path()).unwrap().to_str().unwrap())));
    let readme = std::fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert!(readme.contains("Awaiting") && !readme.contains("0 passed"));
    let mut bench = record(3, "benchmark", DATE, 1);
    bench["data"]["charts"] = json!([{"name":"latency-p50","title":"Synthetic renderer data — Median latency","unit":"Microseconds — lower is better","note":"Test fixture, not a performance claim.","panels":[{"title":"65536 byte payload","bars":[{"label":"gRPC (tonic)","value":10.0},{"label":"WebSockets","value":null}]}]}]);
    let history = render::merge(
        &render::merge(&render::empty_history(), &record(1, "full", DATE, 1)).unwrap(),
        &bench,
    )
    .unwrap();
    render::render(template, &history, dir.path()).unwrap();
    let svg = std::fs::read_to_string(dir.path().join("docs/reports/latency-p50.svg")).unwrap();
    let document = roxmltree::Document::parse(&svg).unwrap();
    let visible = document
        .descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect::<String>();
    for label in [
        "gRPC (tonic)",
        "WebSockets",
        "10.00",
        "65536 byte payload",
        "Microseconds",
        "N/A",
    ] {
        assert!(visible.contains(label), "missing SVG label: {label}");
    }
    roxmltree::Document::parse(&svg).unwrap();
    let path = dir.path().join("docs/reports/test-history.svg");
    let before = std::fs::read(&path).unwrap();
    render::render(template, &history, dir.path()).unwrap();
    assert_eq!(before, std::fs::read(path).unwrap());
    render::render(template, &render::empty_history(), dir.path()).unwrap();
    assert!(!dir.path().join("docs/reports/latency-p50.svg").exists());
}
#[test]
fn frozen_history_remains_readable() {
    let history = capntproto_dev::read_json(
        capntproto_dev::root().join("quality/reporting/history-seed.json"),
    )
    .unwrap();
    render::validate_history(&history).unwrap();
}

#[test]
fn report_text_cannot_create_markdown_links_or_headings() {
    let dir = tempfile::tempdir().unwrap();
    let mut data = publication("fuzz");
    data["note"] = json!("[open](https://example.invalid)\n# injected <script>");
    render::artifact(&data, dir.path()).unwrap();
    let readme = std::fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert!(!readme.contains("[open]"));
    assert!(!readme.contains("\n# injected"));
    assert!(!readme.contains("<script>"));
}

#[test]
fn rendering_cannot_overwrite_source_through_parent_alias() {
    let dir = tempfile::tempdir().unwrap();
    capntproto_dev::write(dir.path().join("README.md"), "source").unwrap();
    let error = capntproto_dev::reports::run(capntproto_dev::reports::Args::Render {
        root: Some(dir.path().into()),
        output: dir.path().join("nested/.."),
        history: None,
    })
    .unwrap_err();
    assert!(error.to_string().contains("separate output"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
        "source"
    );
}
