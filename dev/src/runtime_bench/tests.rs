use super::*;
fn trials(smoke: bool, repetitions: usize) -> Vec<Trial> {
    plan(smoke).into_iter().flat_map(|cell|(0..repetitions).map(move|repetition|{
        let mut sample=json!({"wall_ns":cell.iterations*100+repetition as u64,"cpu_ns":cell.iterations*90,"cycles":null,"cycles_error":"perf unavailable in fixture","blocks":cell.iterations*7,"bytes":cell.iterations*256,"reads":cell.iterations*2,"writes":cell.iterations*3,"received_bytes":cell.iterations*64,"transmitted_bytes":cell.iterations*64,"callgrind_markers":true});
        if repetition==2 {sample["blocks"]=json!(cell.iterations*5);}
        Trial{output:json!({"version":1,"runtime":cell.runtime,"transport":cell.transport,"driver":cell.driver,"case":cell.case,"iterations":cell.iterations,"warmup":cell.warmup,"io_level":cell.io_level(),"sample":sample}),cell:cell.clone(),repetition}
    })).collect()
}
#[test]
fn matrix_covers_every_runtime_transport_driver_and_fixture() {
    let plan = plan(false);
    assert_eq!(plan.len(), 8);
    assert_eq!(plan.iter().collect::<BTreeSet<_>>().len(), 8);
    for runtime in RUNTIMES {
        assert_eq!(plan.iter().filter(|c| c.runtime == runtime).count(), 8);
    }
    assert_eq!(
        plan.iter().filter(|c| c.case == "fanout-128").count(),
        RUNTIMES.len() * TRANSPORTS.len() * DRIVERS.len()
    );
    assert!(plan.iter().any(|c| c.runtime == "tokio"
        && c.transport == "tls"
        && c.driver == "legacy"
        && c.case == "deep-32-concurrent-32"));
}
#[test]
fn minima_are_independent_and_spread_uses_all_five_processes() {
    let rows = aggregate(&trials(false, 5), false, 5, &BTreeMap::new()).unwrap();
    for row in rows {
        let blocks = row.metrics["blocks"].as_ref().unwrap();
        assert_eq!(blocks.min, 5.);
        assert_eq!(blocks.mean, 6.6);
        assert!(blocks.sd > 0.);
        assert_eq!(blocks.max, 7.);
        assert!(row.metrics["cycles"].is_none());
    }
    let s = stats(&[1., 2., 3., 4., 5.]);
    assert_eq!(s.mean, 3.);
    assert_eq!(s.min, 1.);
    assert!((s.sd - 2.5f64.sqrt()).abs() < 1e-12);
}
#[test]
fn incomplete_duplicate_mislabelled_and_wrong_budget_trials_are_rejected() {
    let valid = trials(false, 5);
    for edit in 0..8 {
        let mut bad = valid.clone();
        match edit {
            0 => {
                bad.pop();
            }
            1 => bad.push(valid[0].clone()),
            2 => bad[0].output["driver"] = json!("other"),
            3 => bad[0].output["io_level"] = json!("udp-packets"),
            4 => bad[0].output["iterations"] = json!(1),
            5 => bad[0].output["sample"]["wall_ns"] = json!(0),
            6 => bad[0].output["sample"]["blocks"] = json!(-1),
            _ => bad[0].output["sample"]["cycles_error"] = Value::Null,
        }
        assert!(
            aggregate(&bad, false, 5, &BTreeMap::new()).is_err(),
            "accepted mutation {edit}"
        );
    }
    assert!(aggregate(&valid, false, 1, &BTreeMap::new()).is_err());
    assert!(aggregate(
        &valid,
        false,
        5,
        &BTreeMap::from([("monoio-uring".into(), "EPERM".into())])
    )
    .is_err());
}
#[test]
fn unsupported_runtime_is_rejected_even_for_smoke() {
    let samples = trials(true, 1);
    assert!(aggregate(
        &samples,
        true,
        1,
        &BTreeMap::from([("retired".into(), "unsupported".into())])
    )
    .is_err());
}
pub(crate) fn fixture(directory: &Path) -> Value {
    fs::create_dir_all(directory).unwrap();
    let trials = trials(false, 5);
    let mut raw = fs::File::create(directory.join("trials.jsonl")).unwrap();
    for trial in &trials {
        serde_json::to_writer(&mut raw, trial).unwrap();
        writeln!(raw).unwrap();
    }
    let summary = Summary {
        version: 1,
        mode: "native".into(),
        status: "passed".into(),
        source_id: Some("a".repeat(64)),
        certificates: BTreeMap::from([
            ("ca.der".into(), "e".repeat(64)),
            ("server.der".into(), "f".repeat(64)),
        ]),
        worker_sha256: "b".repeat(64),
        runner_sha256: "c".repeat(64),
        trials_sha256: Some(crate::file_hash(directory.join("trials.jsonl")).unwrap()),
        repetitions: 5,
        rows: aggregate(&trials, false, 5, &BTreeMap::new()).unwrap(),
        unsupported: BTreeMap::new(),
        error: None,
    };
    crate::write_json(directory.join("summary.json"), &summary).unwrap();
    json!({"source_id":"a".repeat(64),"security_fixture":{"certificate_sha256":{"ca.der":"e".repeat(64),"server.der":"f".repeat(64)}},"binaries":{"runtime-bench":"b".repeat(64),"runtime-runner":"c".repeat(64)}})
}
#[test]
fn publication_recomputes_every_cell_and_rejects_stale_sources_or_forged_summaries() {
    let temp = tempfile::tempdir().unwrap();
    let manifest = fixture(temp.path());
    assert_eq!(verified(temp.path(), &manifest).unwrap().rows.len(), 8);
    let base = crate::read_json(temp.path().join("summary.json")).unwrap();
    for (pointer, value) in [
        ("/mode", json!("smoke")),
        ("/certificates/ca.der", json!("d".repeat(64))),
        ("/source_id", json!("d".repeat(64))),
        ("/worker_sha256", json!("d".repeat(64))),
        ("/runner_sha256", json!("d".repeat(64))),
        ("/rows/0/metrics/cpu_ns/min", json!(1.)),
        ("/status", json!("failed")),
        ("/trials_sha256", json!("d".repeat(64))),
    ] {
        let mut bad = base.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        crate::write_json(temp.path().join("summary.json"), &bad).unwrap();
        assert!(
            verified(temp.path(), &manifest).is_err(),
            "accepted {pointer}"
        );
    }
}
#[test]
fn report_export_includes_all_eight_tls_fixtures() {
    let temp = tempfile::tempdir().unwrap();
    let manifest = fixture(&temp.path().join("benchmark/secure/secure/runtimes"));
    let path = temp.path().join("manifest.json");
    crate::write_json(&path, &manifest).unwrap();
    let publication = crate::reports::benchmarks::collect(temp.path(), Some(&path), {
        let mut base = crate::reports::data::unavailable("benchmark", "fixture");
        base["status"] = json!("passed");
        base
    })
    .unwrap();
    assert_eq!(publication["status"], "passed");
    assert_eq!(
        publication["tables"][0]["rows"].as_array().unwrap().len(),
        8
    );
    crate::reports::data::validate_publication(&publication).unwrap();
}

#[test]
fn runtime_success_cannot_override_missing_or_failed_base_campaigns() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("manifest.json");
    let runtime = temp.path().join("benchmark/secure/secure/runtimes");
    crate::write_json(&path, &fixture(&runtime)).unwrap();
    for status in ["unavailable", "failed"] {
        let mut base = crate::reports::data::unavailable("benchmark", "base campaign missing");
        base["status"] = json!(status);
        let result = crate::reports::benchmarks::collect(temp.path(), Some(&path), base).unwrap();
        assert_eq!(result["status"], status);
    }
    let mut summary = crate::read_json(runtime.join("summary.json")).unwrap();
    summary["status"] = json!("failed");
    crate::write_json(runtime.join("summary.json"), &summary).unwrap();
    let mut base = crate::reports::data::unavailable("benchmark", "base passed");
    base["status"] = json!("passed");
    let result = crate::reports::benchmarks::collect(temp.path(), Some(&path), base).unwrap();
    assert_eq!(result["status"], "failed");
    assert!(result["tables"].is_null());
}
