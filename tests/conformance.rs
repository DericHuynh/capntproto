//! Independent requirements and explicit wire/local-state projection checks.
use capntproto_test_support::verification::{command, root, run};
use std::fs;
fn check(name: &str) {
    let work = tempfile::tempdir().unwrap();
    for file in [
        "verification/CapnpNetwork.tla",
        "verification/CapnpNetworkChecks.tla",
        "verification/audit/CapnpConformance.tla",
    ] {
        fs::copy(
            root().join(file),
            work.path()
                .join(std::path::Path::new(file).file_name().unwrap()),
        )
        .unwrap();
    }
    fs::copy(
        root().join(format!("verification/audit/configs/{name}.cfg")),
        work.path().join("model.cfg"),
    )
    .unwrap();
    let java = std::env::var("JAVA").unwrap_or_else(|_| {
        if std::path::Path::new("/tmp/capntproto-jre17/bin/java").exists() {
            "/tmp/capntproto-jre17/bin/java".into()
        } else {
            "java".into()
        }
    });
    let jar = std::env::var("TLA2TOOLS_JAR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            if root().join("target/tools/tla2tools.jar").is_file() {
                root().join("target/tools/tla2tools.jar")
            } else {
                "/tmp/capntproto-tla2tools.jar".into()
            }
        });
    let output = run(
        command(java)
            .args(["-XX:+UseParallelGC", "-Xmx1g", "-cp"])
            .arg(jar)
            .args(["tlc2.TLC", "-workers", "1", "-fp", "0", "-metadir"])
            .arg(work.path().join("states"))
            .args(["-config", "model.cfg", "CapnpConformance"])
            .current_dir(work.path()),
        &root().join(format!("target/verification/conformance/{name}.log")),
        0,
    )
    .unwrap();
    assert!(output.contains("Model checking completed. No error has been found."));
}
#[test]
fn automatic_handoff() {
    check("AutomaticHandoff");
}
#[test]
fn automatic_handoff_settles() {
    check("AutomaticHandoffSettles");
}
#[test]
fn provider_capability_survives() {
    check("ProviderCapabilitySurvives");
}
#[test]
fn scripted_handoff() {
    check("ScriptedHandoff");
}
#[test]
fn generated_release_effect() {
    check("GeneratedReleaseEffect");
}
#[test]
fn equal_join() {
    check("EqualJoin");
}
#[test]
fn unequal_join() {
    check("UnequalJoin");
}
#[test]
fn wire_release_effect() {
    check("WireReleaseEffect");
}
#[test]
fn wire_request_method() {
    check("WireRequestMethod");
}

#[test]
fn generated_wire_relation() {
    check("GeneratedWireRelation");
}
#[test]
fn wire_release_isolation() {
    check("WireReleaseIsolation");
}
#[test]
fn wire_request_data() {
    check("WireRequestData");
}
