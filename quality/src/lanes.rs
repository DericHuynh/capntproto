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

/// The ordinary CI partition, independent of LLVM instrumentation and floors.
pub fn cargo(r: &mut Runner) -> Result<()> {
    let filter = format!(
        "not ({})",
        CARGO_SKIPS
            .iter()
            .map(|name| format!("test({name})"))
            .collect::<Vec<_>>()
            .join(" | ")
    );
    let inventory = r.run_stdout(
        "workspace-inventory",
        v::command("cargo").args([
            "auditable",
            "nextest",
            "list",
            "--locked",
            "--workspace",
            "--all-targets",
            "--features",
            "capntproto/quic",
            "--message-format=json",
        ]),
    )?;
    let inventory_path = r.directory.join("workspace-inventory.json");
    std::fs::write(&inventory_path, &inventory)?;
    // Build the service workspace before concurrent tests so cold QUIC builds
    // do not consume the bounded service-test subprocess deadline.
    r.run(
        "service-examples-build",
        v::command("cargo")
            .args([
                "auditable",
                "build",
                "--locked",
                "--manifest-path",
                "examples/kv/Cargo.toml",
                "--bins",
                "--features",
                "quic",
                "--target-dir",
            ])
            .arg(v::root().join("target/service-examples")),
    )?;
    let tests = r.nextest(
        "workspace-tests",
        v::command("cargo")
            .args([
                "auditable",
                "nextest",
                "run",
                "--locked",
                "--workspace",
                "--all-targets",
                "--features",
                "capntproto/quic",
                "--no-fail-fast",
                "-E",
                &filter,
            ])
            .env("CAPNTPROTO_CI_LANE", "cargo")
            .env("CAPNTPROTO_TEST_INVENTORY", &inventory_path)
            .env("CAPNTPROTO_TEST_INVENTORY_SHA256", v::sha256(&inventory)),
    );
    // Use the existing pinned native verification nightly for libtest JSON.
    // Always retain doctest results even after an ordinary test failure.
    let docs = r.cargo(
        "workspace-doctests",
        &[
            "+nightly-2026-08-29",
            "test",
            "--locked",
            "--ignore-rust-version",
            "--workspace",
            "--features",
            "capntproto/quic",
            "--doc",
            "--no-fail-fast",
            "--",
            "-Z",
            "unstable-options",
            "--format=json",
        ],
    );
    tests?;
    docs?;
    Ok(())
}

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
                "--features",
                "capntproto/quic",
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
        let host = r
            .evidence
            .compiler
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .ok_or("rustc did not report its host target")?
            .to_owned();
        let binaries = std::path::Path::new("target/afl-build")
            .join(&host)
            .join("debug");
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
                // Explicit target selection confines AFL's RUSTFLAGS to the
                // fuzz targets. Schema compilers and build scripts run on the
                // host and must not execute AFL-instrumented code at build time.
                .args(["--target", &host])
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
                .arg("--binaries")
                .arg(&binaries)
                .arg("--corpus")
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
