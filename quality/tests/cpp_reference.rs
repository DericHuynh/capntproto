//! Full native reference suites are part of the ordinary workspace test command.
use reproto_quality::{coverage, Runner};
use reproto_test_support::verification as v;
use std::{fs, path::PathBuf};
#[test]
fn complete_cpp_reference_suites() {
    let native_profiles = tempfile::tempdir().unwrap();
    let profiles = std::env::var_os("REPROTO_FULL_COVERAGE_PROFILES")
        .map(PathBuf::from)
        .unwrap_or_else(|| native_profiles.path().to_owned());
    let output = std::env::var_os("REPROTO_FULL_COVERAGE_REPORT")
        .map(PathBuf::from)
        .unwrap_or_else(|| v::root().join("target/verification/cpp-complete"));
    let mut runner = Runner::new("cpp-reference", &output).unwrap();
    let result = coverage::cpp(&mut runner, &profiles).and_then(|objects| {
        fs::write(
            profiles.join("cpp-objects.json"),
            serde_json::to_vec(&objects)?,
        )?;
        Ok(())
    });
    runner.finish(result).unwrap();
}
