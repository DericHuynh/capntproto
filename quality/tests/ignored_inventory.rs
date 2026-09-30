//! Listing ignored controls does not execute them or repeat the workspace suite.
use reproto_test_support::verification as v;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};
#[test]
fn ignored_controls_have_reviewed_reasons() {
    let output = v::run(
        std::process::Command::new("cargo")
            .current_dir(v::root())
            .args([
                "test",
                "--locked",
                "--ignore-rust-version",
                "--workspace",
                "--all-targets",
                "--",
                "--ignored",
                "--list",
            ]),
        &v::root().join("target/verification/ignored-inventory.log"),
        0,
    )
    .unwrap();
    let actual: BTreeSet<_> = output
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .map(str::to_owned)
        .collect();
    let reviewed: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(v::root().join("quality/ignored-tests.json")).unwrap())
            .unwrap();
    assert_eq!(actual, reviewed.keys().cloned().collect());
    assert!(reviewed.values().all(|reason| !reason.trim().is_empty()));
}
