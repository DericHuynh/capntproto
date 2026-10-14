//! Listing ignored controls does not execute them or repeat the workspace suite.
use capntproto_test_support::verification as v;
use std::{collections::BTreeMap, fs, time::Duration};
#[test]
fn ignored_controls_have_reviewed_reasons() {
    if let Some(path) = std::env::var_os("CAPNTPROTO_TEST_INVENTORY") {
        let bytes = fs::read(path).unwrap();
        assert_eq!(
            v::sha256(&bytes),
            std::env::var("CAPNTPROTO_TEST_INVENTORY_SHA256").unwrap()
        );
        verify_inventory(std::str::from_utf8(&bytes).unwrap());
        return;
    }
    // A cold isolated workspace build (including quiche) can exceed the usual
    // ten-minute subprocess deadline with two Cargo build jobs. Keep a
    // bounded build budget without skipping inventory or sharing executables.
    let log = v::root().join("target/verification/ignored-inventory.log");
    let output = v::run_stdout_with_timeout(
        v::command("cargo")
            // Listing all targets builds a different feature/profile graph.
            // Cargo may replace test executables still queued by the outer
            // nextest process, so inventory compilation owns its output tree.
            .env(
                "CARGO_TARGET_DIR",
                v::root().join("target/verification/ignored-inventory-build"),
            )
            .args([
                "nextest",
                "list",
                "--locked",
                "--ignore-rust-version",
                "--workspace",
                "--all-targets",
                "--message-format=json",
            ]),
        &log.with_extension("stdout"),
        &log,
        0,
        Duration::from_secs(30 * 60),
    )
    .unwrap();
    verify_inventory(&output);
}

fn verify_inventory(output: &str) {
    let actual = v::nextest::inventory(output, true);
    let reviewed: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(v::root().join("quality/ignored-tests.json")).unwrap())
            .unwrap();
    assert_eq!(actual, reviewed.keys().cloned().collect());
    assert!(reviewed.values().all(|reason| !reason.trim().is_empty()));
}
