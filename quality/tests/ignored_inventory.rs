//! Listing ignored controls does not execute them or repeat the workspace suite.
use capntproto_test_support::verification as v;
use std::{collections::BTreeMap, fs};
#[test]
fn ignored_controls_have_reviewed_reasons() {
    let output = v::run_stdout(
        std::process::Command::new("cargo")
            .current_dir(v::root())
            .env_remove("NEXTEST_PROFILE")
            .args([
                "nextest",
                "list",
                "--locked",
                "--ignore-rust-version",
                "--workspace",
                "--all-targets",
                "--message-format=json",
            ]),
        &v::root().join("target/verification/ignored-inventory.log"),
        0,
    )
    .unwrap();
    let actual = v::nextest::inventory(&output, true);
    let reviewed: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(v::root().join("quality/ignored-tests.json")).unwrap())
            .unwrap();
    assert_eq!(actual, reviewed.keys().cloned().collect());
    assert!(reviewed.values().all(|reason| !reason.trim().is_empty()));
}
