//! CI partitions; `cargo nextest run --workspace` continues to run the complete local suite.
use crate::{Result, Runner};
use capntproto_test_support::verification as v;

pub const CARGO_SKIPS: &[&str] = &[
    "tlc",
    "native_fuzz_smoke",
    "serialization_and_ownership_miri",
    "authority_and_transition_mutations",
    "authority_and_transition_coverage",
];

pub fn models(r: &mut Runner) -> Result<()> {
    let measurements = r.directory.join("models");
    if measurements.exists() {
        std::fs::remove_dir_all(&measurements)?;
    }
    std::fs::create_dir_all(&measurements)?;
    let session = tempfile::Builder::new()
        .prefix("tlc-session-")
        .tempdir_in(&r.directory)?;
    let result = r.nextest(
        "model-tests",
        v::command("cargo")
            .args([
                "+nightly-2026-08-29",
                "auditable",
                "nextest",
                "run",
                "--locked",
                "--workspace",
                "--no-fail-fast",
                "tlc",
            ])
            .env("CAPNTPROTO_CI_LANE", "models")
            .env_remove("CAPNTPROTO_TLC_FRESH")
            .env("CAPNTPROTO_TLC_SESSION", session.path())
            .env_remove("CAPNTPROTO_TLC_CASES")
            .env("CAPNTPROTO_TLC_REPORT", &measurements),
    );
    let mut models = Vec::new();
    for entry in std::fs::read_dir(measurements)? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
            value["id"] = path.file_stem().unwrap().to_string_lossy().as_ref().into();
            models.push(value);
        }
    }
    models.sort_by_key(|value| value["id"].as_str().unwrap().to_owned());
    r.evidence.data["models"] = serde_json::json!(models);
    r.save()?;
    result?;
    if models.is_empty() {
        return Err("model job produced no TLC measurements".into());
    }
    Ok(())
}

/// Keep independent engines running after a finding so both retain diagnostics.
pub fn fuzz(r: &mut Runner) -> Result<()> {
    let libfuzzer = r.cargo(
        "libfuzzer",
        &["nextest", "run", "--locked", "--test", "native_fuzz"],
    );
    if libfuzzer.is_ok() {
        let bytes = std::fs::read(v::root().join("target/verification/native-fuzz/checked.json"))?;
        r.evidence.data["libfuzzer"] = serde_json::from_slice(&bytes)?;
    }
    let corpus = r.directory.join("afl-corpus");
    let afl_output = r.directory.join("afl");
    // Only owned, generated directories are cleared; retained findings are
    // uploaded by CI before a later run reuses the output path.
    for path in [&corpus, &afl_output] {
        if path.exists() {
            std::fs::remove_dir_all(path)?;
        }
    }
    let afl = (|| -> Result<()> {
        r.run(
            "afl-build",
            v::command("cargo")
                .args([
                    "afl",
                    "build",
                    "--locked",
                    "--manifest-path",
                    "fuzz/Cargo.toml",
                    "--no-default-features",
                    "--features",
                    "afl-targets",
                    "--bins",
                    "--target-dir",
                    "target/afl-build",
                ])
                .env("AFL_NO_CFG_FUZZING", "1"),
        )?;
        r.cargo(
            "afl-seeds",
            &[
                "run",
                "--locked",
                "--manifest-path",
                "fuzz/Cargo.toml",
                "--no-default-features",
                "--example",
                "afl_seeds",
                "--",
                corpus.to_str().ok_or("non-UTF8 corpus path")?,
            ],
        )?;
        let result = r.run(
            "afl-campaigns",
            v::command("cargo")
                .args(["run", "--locked", "-p", "capntproto-dev", "--", "afl-fuzz"])
                .arg("--output")
                .arg(&afl_output)
                .args(["--binaries", "target/afl-build/debug", "--corpus"])
                .arg(&corpus),
        );
        if let Ok(bytes) = std::fs::read(afl_output.join("afl.json")) {
            r.evidence.data["afl"] = serde_json::from_slice(&bytes)?;
        }
        result?;
        Ok(())
    })();
    r.save()?;
    libfuzzer?;
    afl
}
