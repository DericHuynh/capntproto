use super::*;
#[test]
fn nextest_reports_are_distinct_and_failures_propagate() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path().join(".config/nextest.toml"),
        "[profile.ci.junit]\npath=\"junit.xml\"\n",
    )
    .unwrap();
    let mut paths = Vec::new();
    for case in ["one", "two", "one"] {
        let code = nextest_at(
            dir.path(),
            &["cargo".into(), "--".into(), "--test".into(), case.into()],
            |command| {
                assert_eq!(command.get_current_dir(), Some(dir.path()));
                let args: Vec<_> = command.get_args().collect();
                let config =
                    Path::new(args[args.iter().position(|a| *a == "--config-file").unwrap() + 1]);
                let body = fs::read_to_string(config).unwrap();
                let line = body
                    .lines()
                    .filter_map(|l| l.strip_prefix("path = "))
                    .next_back()
                    .unwrap();
                let report: PathBuf = serde_json::from_str(line).unwrap();
                assert!(!report.exists());
                write(&report, "<testsuites/>").unwrap();
                paths.push(report);
                Ok(100)
            },
        )
        .unwrap();
        assert_eq!(code, 100);
    }
    assert_ne!(paths[0], paths[1]);
    assert_eq!(paths[0], paths[2]);
    assert!(paths.iter().all(|p| p.exists()));
}
#[test]
fn nextest_success_without_junit_fails() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path().join(".config/nextest.toml"), "").unwrap();
    let error = nextest_at(
        dir.path(),
        &[
            "cargo".into(),
            "+nightly".into(),
            "careful".into(),
            "--".into(),
            "--lib".into(),
        ],
        |_| Ok(0),
    )
    .unwrap_err();
    assert!(error.to_string().contains("no JUnit"));
}
#[test]
fn rust_unsafe_diagnostics_are_source_bound_and_deduplicated() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path().join("unsafe.rs"), "unsafe fn f() {}").unwrap();
    let diagnostic = json!({"reason":"compiler-message","message":{"code":{"code":"E0133"},"spans":[{"file_name":"unsafe.rs","is_primary":true,"line_start":1,"column_start":1}]}});
    let actual = unsafe_inventory(
        &format!("{diagnostic}\n{diagnostic}\ncargo progress"),
        dir.path(),
    )
    .unwrap();
    assert_eq!(
        actual["unsafe.rs"]["diagnostics"],
        json!({"unsafe_op_in_unsafe_fn":1})
    );
    assert_eq!(
        actual["unsafe.rs"]["sha256"],
        file_hash(dir.path().join("unsafe.rs")).unwrap()
    );
}
#[test]
fn unsafe_debt_cannot_grow_change_or_cover_application_code() {
    let path = "crates/capntproto-core/src/private/layout.rs";
    let item = json!({"sha256":"old-source","diagnostics":{"unsafe_op_in_unsafe_fn":1}});
    let actual = json!({path:item});
    let baseline = json!({"files":actual});
    validate_unsafe(&actual, &baseline).unwrap();
    for invalid in [
        json!({}),
        json!({path:{"sha256":"changed-source","diagnostics":{"unsafe_op_in_unsafe_fn":1}}}),
        json!({path:{"sha256":"old-source","diagnostics":{}}}),
        json!({path:item,"src/rpc/tls.rs":item}),
    ] {
        assert!(validate_unsafe(&invalid, &baseline).is_err());
    }
    assert!(validate_unsafe(&json!({}), &json!({"files":{"src/storage.rs":{}}})).is_err());
}
#[test]
fn repository_rejects_tracked_ignored_files() {
    let dir = tempfile::tempdir().unwrap();
    process::checked(Command::new("git").arg("init").arg(dir.path()), 30).unwrap();
    write(dir.path().join(".gitignore"), "generated\n").unwrap();
    write(dir.path().join("generated"), "must not be tracked").unwrap();
    process::checked(
        Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["add", "-f", "generated"]),
        30,
    )
    .unwrap();
    assert!(repository(dir.path())
        .unwrap_err()
        .to_string()
        .contains("Ignored file is tracked"));
}
#[test]
fn workflows_keep_unique_trigger_owners() {
    workflows().unwrap();
    let text = |name: &str| {
        fs::read_to_string(root().join(".github/workflows").join(format!("{name}.yml"))).unwrap()
    };
    assert!(text("verification-tests").contains("--kind cargo"));
    assert!(!text("verification-tests").contains("native_fuzz"));
    assert!(text("verification-models").contains("-- models target/quality/models"));
    assert!(text("verification-fuzz").contains("-- fuzz target/quality/fuzz"));
    assert!(text("verification-extended").contains("--test guard_quality"));
    let policy = fs::read_to_string(root().join("quality/src/lanes.rs")).unwrap();
    for name in [
        "tlc",
        "native_fuzz_smoke",
        "serialization_and_ownership_miri",
        "authority_and_transition_mutations",
        "authority_and_transition_coverage",
    ] {
        assert!(policy.contains(&format!("\"{name}\"")));
    }
    assert!(fs::read_to_string(root().join("quality/src/coverage.rs"))
        .unwrap()
        .contains("crate::lanes::CARGO_SKIPS"));
}
