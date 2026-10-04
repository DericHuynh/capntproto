//! Bounded wire/real-crypto fuzz qualification orchestrated entirely by Cargo tests.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
use capntproto_test_support::verification::{self as v, command, root, run};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs};

const NIGHTLY: &str = "+nightly-2026-08-29";

fn input_hashes() -> BTreeMap<String, String> {
    v::distribution::sources(&root())
        .unwrap()
        .into_iter()
        .filter(|path| {
            path.starts_with("fuzz")
                || path.starts_with("src")
                || path.starts_with("crates/capntproto-core/src")
                || path.starts_with("crates/capntproto-rpc")
                || path.starts_with("crates/capntproto-futures")
                || path.starts_with("crates/capntproto-codegen")
                || [
                    "crates/capntproto-core/Cargo.toml",
                    "tests/native_fuzz.rs",
                    "tests/schema_loader.rs",
                    "Cargo.lock",
                    "Cargo.toml",
                    "rust-toolchain.toml",
                    ".cargo/config.toml",
                ]
                .iter()
                .any(|file| path == std::path::Path::new(file))
        })
        .map(|path| {
            let hash = v::sha256(fs::read(root().join(&path)).unwrap());
            (path.to_string_lossy().into_owned(), hash)
        })
        .collect()
}

#[test]
fn native_fuzz_smoke() {
    assert_ne!(
        std::env::var("CAPNTPROTO_CI_LANE").as_deref(),
        Ok("cargo"),
        "fuzz campaigns belong to the fuzz job"
    );
    let base = root().join("target/verification/native-fuzz");
    fs::create_dir_all(&base).unwrap();
    // An interrupted or failed qualification must not leave a stale success.
    let checked = base.join("checked.json");
    if checked.exists() {
        fs::remove_file(&checked).unwrap();
    }
    let directory = tempfile::tempdir_in(&base).unwrap().keep();
    let inputs = input_hashes();
    let wire_runs = match std::env::var("CAPNTPROTO_FUZZ_WIRE_RUNS") {
        Ok(value) => value
            .parse::<u32>()
            .expect("CAPNTPROTO_FUZZ_WIRE_RUNS must be an integer"),
        Err(std::env::VarError::NotPresent) => 16384,
        Err(error) => panic!("CAPNTPROTO_FUZZ_WIRE_RUNS: {error}"),
    };
    assert!(
        wire_runs >= 16384,
        "wire campaign cannot reduce the smoke budget"
    );
    let log = |name: &str| directory.join(format!("{name}.log"));
    let cargo =
        |args: &[&str], name: &str| run(command("cargo").args(args), &log(name), 0).unwrap();
    let lock_before = fs::read(root().join("fuzz/Cargo.lock")).unwrap();
    let version = cargo(&["fuzz", "--version"], "cargo-fuzz-version");
    assert_eq!(version.trim(), "cargo-fuzz 0.13.1");
    let compiler = run(
        command("rustc").args([NIGHTLY, "--version", "--verbose"]),
        &log("compiler"),
        0,
    )
    .unwrap();
    let schema_compiler = run(
        command("capnp").arg("--version"),
        &log("schema-compiler"),
        0,
    )
    .unwrap();

    cargo(
        &["fmt", "--manifest-path", "fuzz/Cargo.toml", "--", "--check"],
        "format",
    );
    cargo(
        &[
            "clippy",
            "--locked",
            "--manifest-path",
            "fuzz/Cargo.toml",
            "--no-default-features",
            "--lib",
            "--tests",
            "--",
            "-D",
            "warnings",
        ],
        "clippy",
    );
    let features = cargo(
        &[
            "tree",
            "--locked",
            "--manifest-path",
            "fuzz/Cargo.toml",
            "-e",
            "features",
            "-i",
            "quiche",
        ],
        "features",
    );
    assert!(features.contains("quiche feature \"boringssl-boring-crate\""));
    for excluded in ["fuzzing", "ffi", "internal"] {
        assert!(!features.contains(&format!("quiche feature \"{excluded}\"")));
    }
    let corpus = directory.join("corpus");
    let unit_output = run(
        command("cargo")
            .env("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1")
            .args([
                "nextest",
                "run",
                "--message-format=libtest-json-plus",
                "--locked",
                "--manifest-path",
                "fuzz/Cargo.toml",
                "--no-default-features",
                "--lib",
            ])
            .env("CAPNTPROTO_NATIVE_FUZZ_CORPUS", &corpus)
            .env_remove("CAPNTPROTO_NATIVE_FUZZ_REPLAY"),
        &log("native"),
        0,
    )
    .unwrap();
    assert_eq!(v::nextest::passed(&unit_output).len(), 16);

    let mut campaigns = BTreeMap::new();
    for (target, seed_count, runs) in [
        ("native_packet", 131, 1024),
        ("native_stream", 258, 1024),
        ("capnp_framing", 1478, wire_runs),
        ("capnp_pointers", 1754, wire_runs),
        ("capnp_schema", 13552, wire_runs),
        ("rpc_lifecycle", 592, wire_runs),
    ] {
        let seed_directory = corpus.join(target);
        let seeds: BTreeMap<_, _> = fs::read_dir(&seed_directory)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    v::sha256(fs::read(entry.path()).unwrap()),
                )
            })
            .collect();
        assert_eq!(seeds.len(), seed_count);
        let artifacts = directory.join("artifacts").join(target);
        fs::create_dir_all(&artifacts).unwrap();
        let output = run(
            command("cargo")
                // The pinned sancov/ASan passes leave unresolved __sancov_gen_*
                // symbols under fat LTO. Keep sanitizer coverage enabled and
                // override the checkout's release LTO only for this fuzz build.
                .env("CARGO_PROFILE_RELEASE_LTO", "off")
                .args([
                    NIGHTLY,
                    "fuzz",
                    "run",
                    "--no-cfg-fuzzing",
                    "--sanitizer",
                    "address",
                    "--target",
                    "x86_64-unknown-linux-gnu",
                    "--target-dir",
                    "target/native-fuzz",
                    target,
                ])
                .arg(&seed_directory)
                .arg("--")
                .arg(format!("-runs={runs}"))
                .args([
                    "-max_len=4096",
                    "-seed=1",
                    "-timeout=10",
                    "-rss_limit_mb=1024",
                    "-malloc_limit_mb=128",
                    "-print_final_stats=1",
                ])
                .arg(format!("-artifact_prefix={}/", artifacts.display())),
            &log(target),
            0,
        )
        .unwrap();
        assert!(
            output.contains("inline 8-bit counters"),
            "coverage instrumentation missing"
        );
        let done = output
            .lines()
            .find(|line| line.starts_with('#') && line.contains("\tDONE"))
            .expect("fuzz run did not finish its budget");
        let executed = done
            .strip_prefix('#')
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        // libFuzzer may exceed -runs while initializing/retesting a large seed
        // corpus. Require the entire budget and record actual execution counts.
        assert!(executed >= u64::from(runs), "fuzz budget was not completed");
        let coverage = done
            .split("cov:")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert!(coverage > 0);
        assert!(output.contains(&format!("stat::number_of_executed_units: {executed}\n")));
        assert_eq!(fs::read_dir(&artifacts).unwrap().count(), 0);
        let executable = root()
            .join("target/native-fuzz/x86_64-unknown-linux-gnu/release")
            .join(target);
        campaigns.insert(target, json!({
            "requested_runs": runs, "runs": executed, "initial_seeds": seeds, "coverage_feedback": done,
            "statistics": output.lines().filter(|line| line.starts_with("stat::")).collect::<Vec<_>>(),
            "executable_sha256": v::sha256(fs::read(executable).unwrap()),
        }));
        // Crashes retained by libFuzzer use exactly this ordinary Cargo entry
        // point. Exercise it with a known seed even when no crash was found.
        let replay = run(
            command("cargo")
                .env("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1")
                .args([
                    "nextest",
                    "run",
                    "--message-format=libtest-json-plus",
                    "--locked",
                    "--manifest-path",
                    "fuzz/Cargo.toml",
                    "--no-default-features",
                    "--lib",
                    "tests::replay_saved_fuzz_input",
                    "--",
                    "--exact",
                ])
                .env("CAPNTPROTO_NATIVE_FUZZ_TARGET", target)
                .env(
                    "CAPNTPROTO_NATIVE_FUZZ_REPLAY",
                    seed_directory.join("seed-1"),
                ),
            &log(&format!("{target}-replay")),
            0,
        )
        .unwrap();
        assert_eq!(v::nextest::passed(&replay).len(), 1);
    }
    // cargo-fuzz 0.13.1 has no --locked switch: detect any lockfile drift.
    assert_eq!(
        fs::read(root().join("fuzz/Cargo.lock")).unwrap(),
        lock_before
    );
    let sources = v::distribution::sources(&root()).unwrap();
    for file in [
        "fuzz/Cargo.toml",
        "fuzz/Cargo.lock",
        "fuzz/build.rs",
        "fuzz/schemas/lifecycle.capnp",
        "fuzz/src/lib.rs",
        "fuzz/fuzz_targets/native_packet.rs",
        "fuzz/fuzz_targets/native_stream.rs",
        "fuzz/fuzz_targets/capnp_framing.rs",
        "fuzz/fuzz_targets/capnp_pointers.rs",
        "fuzz/fuzz_targets/capnp_schema.rs",
        "fuzz/fuzz_targets/rpc_lifecycle.rs",
        "fuzz/src/framing.rs",
        "fuzz/src/pointers.rs",
        "fuzz/src/schema.rs",
        "fuzz/src/rpc_lifecycle.rs",
        "fuzz/src/wire.rs",
    ] {
        assert!(
            sources.contains(std::path::Path::new(file)),
            "missing bundled fuzz source: {file}"
        );
    }
    assert!(!sources.iter().any(|p| p.starts_with("fuzz/corpus")
        || p.starts_with("fuzz/artifacts")
        || p.starts_with("fuzz/target")));
    assert_eq!(
        input_hashes(),
        inputs,
        "fuzz sources changed during qualification"
    );
    let metadata: Value = serde_json::from_str(
        &v::run_stdout(
            command("cargo").args([
                "metadata",
                "--locked",
                "--manifest-path",
                "fuzz/Cargo.toml",
                "--format-version",
                "1",
            ]),
            &log("metadata"),
            0,
        )
        .unwrap(),
    )
    .unwrap();
    let provenance = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == "quiche")
        .unwrap();
    assert_eq!(provenance["version"], "0.30.0");
    assert_eq!(
        provenance["source"],
        "registry+https://github.com/rust-lang/crates.io-index"
    );
    let report = json!({
        "format": 3, "cargo_fuzz": version.trim(), "compiler": compiler, "schema_compiler": schema_compiler.trim(), "sanitizer": "address",
        "cfg_fuzzing": false, "quiche": provenance, "inputs": inputs, "campaigns": campaigns,
        "max_input": 4096, "artifacts": directory,
        "scope": "bounded packed/unpacked framing, fragmented I/O, raw and structured pointer mutations, copy/canonicalization and unbound capability indices; schema request mutations, rejection atomicity, typed stub preservation, version replay and bounded reflection/default reads; structured RPC command histories over the production two-party stream reader/dispatcher, reference counts, pending calls, completion/cancellation, ID reuse, promised-answer pipelines, exported promises/outgoing Resolve, terminal malformed messages and teardown; separate Native peers with real mutual TLS with optional admission secrets crypto, Cubic/BBR2, datagram mutations and authenticated streams; RPC fixture has deterministic in-memory I/O and no cryptographic authentication; Native retains real entropy/clocks; no incoming Resolve, general promise graphs, timeout recovery or handoff",
    });
    let bytes = serde_json::to_vec_pretty(&report).unwrap();
    fs::write(directory.join("checked.json"), &bytes).unwrap();
    fs::write(base.join("checked.json"), bytes).unwrap();
    let total_runs: u64 = campaigns
        .values()
        .map(|campaign| campaign["runs"].as_u64().unwrap())
        .sum();
    eprintln!("Fuzz: 6 ASan targets, {total_runs} total runs; native matrices, feature checks and artifact replay passed");
}
