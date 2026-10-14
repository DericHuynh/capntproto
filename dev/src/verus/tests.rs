use super::*;
fn report() -> Value {
    json!({"verus":{"version":VERSION,"commit":COMMIT},
        "verification-results":{"encountered-vir-error":false,"encountered-error":false,"is-verifying-entire-crate":true,"success":true,"verified":1,"errors":0},
        "times-ms":{"smt":{"smt-run-module-times":[{"function-breakdown":[{"function":"f","success":true}]}]}}})
}
#[test]
fn gate_rejects_missing_partial_or_failed_proofs() {
    let expected = BTreeSet::from(["f".to_owned()]);
    validate(&report(), 0, 1, Some(&expected), None).unwrap();
    for (path, value) in [
        (
            "/verification-results/is-verifying-entire-crate",
            json!(false),
        ),
        ("/verification-results/encountered-vir-error", json!(true)),
        ("/verification-results/verified", json!(0)),
        ("/verus/version", json!("different")),
        (
            "/times-ms/smt/smt-run-module-times/0/function-breakdown/0/function",
            json!("other"),
        ),
        (
            "/times-ms/smt/smt-run-module-times/0/function-breakdown/0/success",
            json!(false),
        ),
    ] {
        let mut data = report();
        *data.pointer_mut(path).unwrap() = value;
        assert!(
            validate(&data, 0, 1, Some(&expected), None).is_err(),
            "{path}"
        );
    }
    assert!(validate(&report(), 1, 1, Some(&expected), None).is_err());
    assert!(validate(&report(), 0, 2, Some(&expected), None).is_err());
}
#[test]
fn controls_must_fail_the_intended_logical_function() {
    let mut data = report();
    let expected = BTreeSet::from(["f".into()]);
    assert!(validate(&data, 1, 1, Some(&expected), Some("f")).is_err());
    data["verification-results"]["success"] = json!(false);
    data["verification-results"]["errors"] = json!(1);
    *data
        .pointer_mut("/times-ms/smt/smt-run-module-times/0/function-breakdown/0/success")
        .unwrap() = json!(false);
    validate(&data, 1, 1, Some(&expected), Some("f")).unwrap();
    assert!(validate(&data, 1, 1, Some(&expected), Some("unrelated")).is_err());
    assert!(validate(&data, 0, 1, Some(&expected), Some("f")).is_err());
}
#[test]
fn checked_in_plan_matches_actual_mutation_sites() {
    plan(&crate::root()).unwrap();
}
#[test]
fn ambiguous_or_external_mutations_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    crate::write(dir.path().join(SOURCES[1]), "guard guard").unwrap();
    let mut control = json!({"source":SOURCES[1],"from":"guard","to":"broken"});
    assert!(control_source(dir.path(), &control).is_err());
    control["source"] = "../outside.rs".into();
    assert!(control_source(dir.path(), &control).is_err());
}

#[test]
fn loop_obligations_are_additional_to_the_exact_function_inventory() {
    let mut data = report();
    data["verification-results"]["verified"] = json!(2);
    let expected = BTreeSet::from(["f".to_owned()]);
    validate(&data, 0, 1, Some(&expected), None).unwrap();
    data["verification-results"]["verified"] = json!(0);
    assert!(validate(&data, 0, 1, Some(&expected), None).is_err());
}
