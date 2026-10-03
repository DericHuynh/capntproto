//! Independent requirements and explicit wire/local-state projection checks.
use capntproto_test_support::verification::{self as v, Check, Group};
use std::{collections::BTreeMap, fs};
fn check(name: &str) {
    v::verify(&Group {
        id: format!("conformance-{name}"),
        report: format!("conformance/{name}"),
        checks: vec![Check {
            id: name.into(),
            module: "verification/audit/CapnpConformance.tla".into(),
            config: fs::read_to_string(
                v::root().join(format!("verification/audit/configs/{name}.cfg")),
            )
            .unwrap(),
            exit: 0,
            message: "Model checking completed. No error has been found.".into(),
            graph: None,
        }],
        inputs: BTreeMap::new(),
        scope: "independent bounded requirements and wire/local projection".into(),
        limits: serde_json::Value::Null,
    })
    .unwrap();
}
#[test]
fn tlc_automatic_handoff() {
    check("AutomaticHandoff");
}
#[test]
fn tlc_automatic_handoff_settles() {
    check("AutomaticHandoffSettles");
}
#[test]
fn tlc_provider_capability_survives() {
    check("ProviderCapabilitySurvives");
}
#[test]
fn tlc_scripted_handoff() {
    check("ScriptedHandoff");
}
#[test]
fn tlc_generated_release_effect() {
    check("GeneratedReleaseEffect");
}
#[test]
fn tlc_equal_join() {
    check("EqualJoin");
}
#[test]
fn tlc_unequal_join() {
    check("UnequalJoin");
}
#[test]
fn tlc_wire_release_effect() {
    check("WireReleaseEffect");
}
#[test]
fn tlc_wire_request_method() {
    check("WireRequestMethod");
}

#[test]
fn tlc_generated_wire_relation() {
    check("GeneratedWireRelation");
}
#[test]
fn tlc_wire_release_isolation() {
    check("WireReleaseIsolation");
}
#[test]
fn tlc_wire_request_data() {
    check("WireRequestData");
}
