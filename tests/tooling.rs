//! Compiler, distribution and reference checks, all launched by `cargo test`.
use reproto_test_support::verification::{self as v, command, root, run};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeSet, fs, path::PathBuf};

fn log(name: &str) -> PathBuf {
    root()
        .join("target/verification/tooling")
        .join(format!("{name}.log"))
}
fn cargo(args: &[&str], name: &str) -> String {
    cargo_with_toolchain(None, args, name)
}
fn cargo_with_toolchain(toolchain: Option<&str>, args: &[&str], name: &str) -> String {
    let mut cmd = command("cargo");
    if let (Ok(build), Ok(profiles), Ok(flags)) = (
        std::env::var("REPROTO_FULL_COVERAGE_BUILD"),
        std::env::var("REPROTO_FULL_COVERAGE_PROFILES"),
        std::env::var("REPROTO_FULL_COVERAGE_FLAGS"),
    ) {
        if args.first() == Some(&"test") {
            if let Some(manifest) = args.windows(2).find(|p| p[0] == "--manifest-path") {
                cmd.arg("+nightly-2026-03-05")
                    .env(
                        "CARGO_TARGET_DIR",
                        PathBuf::from(build).join(manifest[1].replace('/', "-")),
                    )
                    .env("RUSTFLAGS", &flags)
                    .env("RUSTDOCFLAGS", &flags)
                    .env(
                        "LLVM_PROFILE_FILE",
                        PathBuf::from(profiles).join("%p-%m.profraw"),
                    )
                    .env("REPROTO_COVERAGE_CHILDREN_NATIVE", "1");
                cmd.args(["test", "--ignore-rust-version"]).args(&args[1..]);
                return run(&mut cmd, &log(name), 0).unwrap();
            }
        }
    }
    if let Some(toolchain) = toolchain {
        cmd.arg(toolchain);
    }
    run(cmd.args(args), &log(name), 0).unwrap()
}

#[test]
fn auditable_metadata_is_required_for_binary_artifacts() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    fs::write(
        project.path().join("Cargo.toml"),
        "[package]\nname = \"audit-probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[[bench]]\nname = \"probe\"\nharness = false\n",
    )
    .unwrap();
    let source = project.path().join("src/main.rs");
    fs::write(&source, "fn main() {}\n").unwrap();
    fs::create_dir(project.path().join("benches")).unwrap();
    fs::write(project.path().join("benches/probe.rs"), "fn main() {}\n").unwrap();
    let target = project.path().join("target");
    run(
        command("cargo")
            .args([
                "+1.97.0",
                "auditable",
                "build",
                "--release",
                "--offline",
                "--manifest-path",
            ])
            .arg(project.path().join("Cargo.toml"))
            .arg("--target-dir")
            .arg(&target),
        &log("auditable-build"),
        0,
    )
    .unwrap();
    let binary = target.join(format!(
        "release/audit-probe{}",
        std::env::consts::EXE_SUFFIX
    ));
    let output = run(
        command("cargo")
            .args([
                "+1.97.0",
                "auditable",
                "bench",
                "--locked",
                "--offline",
                "--bench",
                "probe",
                "--no-run",
                "--message-format=json",
                "--manifest-path",
            ])
            .arg(project.path().join("Cargo.toml"))
            .arg("--target-dir")
            .arg(&target),
        &log("auditable-bench"),
        0,
    )
    .unwrap();
    let bench: Value = output
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value["target"]["name"] == "probe" && value["executable"].is_string())
        .expect("Cargo must identify the benchmark executable");
    let inventory = project.path().join("auditable.json");
    run(
        command("python3")
            .args(["scripts/check_auditable.py", "--output"])
            .arg(&inventory)
            .arg(&binary)
            .arg(bench["executable"].as_str().unwrap()),
        &log("auditable-positive"),
        0,
    )
    .unwrap();
    let metadata: Value = serde_json::from_slice(&fs::read(&inventory).unwrap()).unwrap();
    let entry = &metadata[binary.file_name().unwrap().to_str().unwrap()];
    assert_eq!(entry["sha256"], v::sha256(fs::read(&binary).unwrap()));
    assert!(entry["metadata"]["packages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| { p["root"] == true && p["name"] == "audit-probe" && p["version"] == "0.0.0" }));

    // A real unaudited executable is the negative control. Reuse the output
    // path to prove that a failure removes the earlier successful inventory.
    run(
        command("rustc").arg(&source).arg("-o").arg(&binary),
        &log("auditable-negative-build"),
        0,
    )
    .unwrap();
    run(
        command("python3")
            .args(["scripts/check_auditable.py", "--output"])
            .arg(&inventory)
            .arg(&binary),
        &log("auditable-negative"),
        1,
    )
    .unwrap();
    assert!(
        !inventory.exists(),
        "unaudited artifact retained passing evidence"
    );
}

#[test]
fn generated_api_compile_contracts() {
    #[derive(Deserialize)]
    struct Case {
        name: String,
        source: String,
        error: Option<String>,
    }
    let cases: Vec<Case> = serde_json::from_str(include_str!(
        "../test-support/verification/compiler-cases.json"
    ))
    .unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    fs::write(project.path().join("Cargo.toml"), format!("[package]\nname = \"field-api-acceptance\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\ncapnp = {{ path = {:?} }}\n[build-dependencies]\ncapnpc = {{ path = {:?} }}\n", root().join("vendor/capnp"), root().join("vendor/capnpc"))).unwrap();
    fs::write(project.path().join("build.rs"), format!("fn main() {{capnpc::CompilerCommand::new().src_prefix({:?}).file({:?}).import_path({:?}).field_api(true).field_api_values(true).field_api_projections(true).run().unwrap();}}",root().join("schemas"), root().join("schemas/field-api.capnp"), root().join("vendor/capnpc"))).unwrap();
    assert!(cases.iter().any(|c| c.error.is_none()));
    assert!(cases.iter().any(|c| c.error.is_some()));
    // Establish positive controls before interpreting any negative diagnostics.
    for case in cases
        .iter()
        .filter(|c| c.error.is_none())
        .chain(cases.iter().filter(|c| c.error.is_some()))
    {
        fs::write(project.path().join("src/main.rs"), &case.source).unwrap();
        let output = run(
            command("cargo")
                .args(["check", "--offline", "--manifest-path"])
                .arg(project.path().join("Cargo.toml"))
                .arg("--target-dir")
                .arg(root().join("target/field-api-acceptance")),
            &log(&format!("compiler/{}", case.name)),
            if case.error.is_some() { 101 } else { 0 },
        )
        .unwrap();
        if let Some(error) = &case.error {
            assert!(
                output.contains(&format!("error[{error}]")),
                "{}: {output}",
                case.name
            );
        }
    }
}

#[test]
fn pinned_cpp_runtime_reference() {
    v::cpp::reference().unwrap();
}

#[test]
fn installed_cpp_rpc_interoperability() {
    let tmp = tempfile::tempdir().unwrap();
    run(
        command("capnp").args([
            "compile",
            &format!("-oc++:{}", tmp.path().display()),
            "--src-prefix=schemas",
            "schemas/runtime-test.capnp",
            "schemas/cancellation-policy.capnp",
        ]),
        &log("cpp/schema"),
        0,
    )
    .unwrap();
    let flags = run(
        command("pkg-config").args(["--cflags", "--libs", "capnp-rpc"]),
        &log("cpp/flags"),
        0,
    )
    .unwrap();
    for (name, example) in [
        ("rpc-peer", Some("cpp_rpc_interop")),
        ("fd-peer", Some("cpp_fd_interop")),
        ("static-cancellation", None),
    ] {
        let executable = tmp.path().join(name);
        let mut cmd = command("g++");
        cmd.args([
            "-std=c++17",
            &format!("-I{}", tmp.path().display()),
            &format!("tests/cpp/{name}.c++"),
        ])
        .arg(tmp.path().join("runtime-test.capnp.c++"));
        if example.is_none() {
            cmd.arg(tmp.path().join("cancellation-policy.capnp.c++"));
        }
        cmd.arg("-o")
            .arg(&executable)
            .args(flags.split_whitespace());
        run(&mut cmd, &log(&format!("cpp/{name}-compile")), 0).unwrap();
        let mut cmd = if let Some(example) = example {
            let mut c = command("cargo");
            c.args(["run", "--locked", "--quiet", "--example", example, "--"])
                .arg(&executable);
            c
        } else {
            command(&executable)
        };
        run(&mut cmd, &log(&format!("cpp/{name}")), 0).unwrap();
    }
}

#[test]
fn pinned_noise_profile() {
    let pin: Value = serde_json::from_slice(
        &fs::read(root().join("vendor/provenance/snow-revision.json")).unwrap(),
    )
    .unwrap();
    let graph = cargo(
        &["tree", "--locked", "-e", "features", "-i", "snow"],
        "noise/features",
    );
    assert!(graph.contains(pin["revision"].as_str().unwrap()));
    let enabled: BTreeSet<_> = graph
        .lines()
        .filter_map(|l| {
            l.split_once("snow feature \"")
                .and_then(|(_, s)| s.split_once('"').map(|(s, _)| s))
        })
        .collect();
    let mut expected: BTreeSet<_> = pin["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    expected.extend([
        "default-resolver",
        "blake3",
        "chacha20poly1305",
        "curve25519-dalek",
        "getrandom",
    ]);
    assert_eq!(enabled, expected);
    assert_eq!(pin["default_features"], false);
    let runtime = cargo(
        &[
            "tree",
            "--locked",
            "-e",
            "normal,build,features",
            "-i",
            "tokio",
        ],
        "noise/runtime-clock",
    );
    assert!(runtime.contains("quiche feature \"tokio-clock\""));
    assert!(!runtime.contains("tokio feature \"test-util\""));
    let output = cargo(
        &[
            "test",
            "--locked",
            "--manifest-path",
            "vendor/quiche/Cargo.toml",
            "-p",
            "quiche",
            "--no-default-features",
            "--features",
            "noise",
            "--lib",
            "tls::tests::noise_blake3_profile",
            "--",
            "--exact",
        ],
        "noise/backend",
    );
    assert!(output.contains("1 passed; 0 failed"));
    let provenance: Value = serde_json::from_slice(
        &fs::read(root().join("vendor/provenance/quiche-revision.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        v::sha256(fs::read(root().join("vendor/provenance/quiche-noise.patch")).unwrap()),
        provenance["patch_sha256"].as_str().unwrap()
    );
    run(
        command("git")
            .arg("-C")
            .arg(root())
            .args(["apply", "--directory=vendor/quiche", "--reverse", "--check"])
            .arg(root().join("vendor/provenance/quiche-noise.patch")),
        &log("noise/patch"),
        0,
    )
    .unwrap();
}

#[test]
fn quiche_noise_regressions() {
    quiche_regressions("noise,custom-client-dcid,tokio-clock", "noise");
}

fn quiche_regressions(features: &str, report: &str) {
    let base = [
        "test",
        "--locked",
        "--manifest-path",
        "vendor/quiche/Cargo.toml",
        "-p",
        "quiche",
        "--no-default-features",
        "--features",
        features,
        "--lib",
        "--",
    ];
    let mut list = base.to_vec();
    list.extend(["--list", "--format", "terse"]);
    let inventory = cargo(&list, &format!("{report}/inventory"));
    let all: BTreeSet<_> = inventory
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .collect();
    let selected = all.len();
    assert!(
        selected >= 1104,
        "Noise regression selection unexpectedly shrank"
    );
    for required in [
        "noise_tests::",
        "tests::handshake::",
        "tests::update_key_request::",
        "tests::update_key_request_twice_error::",
        "tests::connection_migration::",
        "tests::dgram_multiple_datagrams::",
        "tests::streamio::",
        "tests::handshake_confirmation::",
        "tests::early_1rtt_packet::",
        "tests::limit_handshake_data::",
        "tests::validate_peer_sent_ack_range_for_multi_path::",
        "tests::stop_sending_before_flushed_packets::",
        "tests::pmtud_probe_retry_after_loss::",
        "tests::initial_cwnd::",
        "simulation::packet::",
        "simulation::packet::lifecycle::",
        "simulation::packet::property::",
    ] {
        assert!(
            all.iter().any(|name| name.starts_with(required)),
            "missing critical regression family: {required}"
        );
    }
    let output = cargo(&base, &format!("{report}/regressions"));
    assert!(output.contains(&format!("{selected} passed; 0 failed; 0 ignored")));
    assert!(output.contains("0 filtered out"));
    eprintln!(
        "Noise regression gate ({features}): {selected} passed; no ignored or filtered cases"
    );
}

#[test]
fn quiche_noise_handshake_model() {
    let model = "verification/NoiseHandshakeConfirmation.tla";
    let config = include_str!("../verification/NoiseHandshakeConfirmation.cfg");
    let traces = v::exploration::traces(model, "noise-confirmation", config).unwrap();
    v::exploration::controls(
        model,
        "noise-confirmation",
        config,
        &[
            ("earlyRetire", "RetainUntilProof"),
            ("noRetire", "RetireAfterProof"),
            ("clientRetain", "ClientRetires"),
            ("forgedVerify", "OriginalAddressOnly"),
            ("migratedVerify", "OriginalAddressOnly"),
            ("forgedData", "AuthenticatedOnce"),
            ("duplicateData", "AuthenticatedOnce"),
        ],
        None,
    )
    .unwrap();
    let fields = [
        "event",
        "client",
        "server",
        "clientKeys",
        "serverKeys",
        "verified",
        "otherVerified",
        "confirmed",
        "received",
    ];
    let replay = traces
        .iter()
        .map(|trace| {
            trace
                .iter()
                .map(|state| {
                    fields
                        .iter()
                        .map(|field| state[*field].to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .collect::<Vec<_>>()
                .join(";")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let path = root().join("target/verification/noise-confirmation/traces.txt");
    fs::write(&path, replay).unwrap();
    let output = run(
        command("cargo")
            .args([
                "test",
                "--locked",
                "--manifest-path",
                "vendor/quiche/Cargo.toml",
                "-p",
                "quiche",
                "--no-default-features",
                "--features",
                "noise",
                "--lib",
                "noise_tests::handshake_confirmation_trace_replay",
                "--",
                "--exact",
                "--nocapture",
            ])
            .env("REPROTO_NOISE_CONFIRMATION_TRACES", &path),
        &log("noise/confirmation-replay"),
        0,
    )
    .unwrap();
    assert!(output.contains(&format!(
        "Noise confirmation replay: {} traces",
        traces.len()
    )));
    assert!(output.contains("1 passed; 0 failed"));
}

#[test]
fn quiche_noise_packet_simulation() {
    let destination = root().join("target/verification/noise-simulation");
    fs::create_dir_all(&destination).unwrap();
    let build = cargo(
        &[
            "test",
            "--locked",
            "--manifest-path",
            "vendor/quiche/Cargo.toml",
            "-p",
            "quiche",
            "--no-default-features",
            "--features",
            "noise",
            "--lib",
            "--no-run",
            "--message-format=json",
        ],
        "noise/simulation-build",
    );
    let executables: Vec<PathBuf> = build
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|artifact| {
            artifact["reason"] == "compiler-artifact"
                && artifact["target"]["name"] == "quiche"
                && artifact["profile"]["test"] == true
        })
        .filter_map(|artifact| artifact["executable"].as_str().map(PathBuf::from))
        .collect();
    assert_eq!(executables.len(), 1);
    let binary = fs::read(&executables[0]).unwrap();
    let binary_sha256 = v::sha256(&binary);
    let mut previous: Option<std::collections::BTreeMap<std::ffi::OsString, Vec<u8>>> = None;
    // Independent processes detect entropy/hash-order or wall-clock inputs
    // that an in-process replay can miss. Keep all artifacts for diagnosis.
    for pass in 0..2 {
        let directory = tempfile::tempdir_in(&destination).unwrap().keep();
        let output = run(
            command("cargo")
                .args([
                    "test",
                    "--locked",
                    "--manifest-path",
                    "vendor/quiche/Cargo.toml",
                    "-p",
                    "quiche",
                    "--no-default-features",
                    "--features",
                    "noise",
                    "--lib",
                    "--",
                    "--exact",
                    "simulation::packet::seeded_packet_replay",
                    "simulation::packet::lifecycle::lifecycle_packet_replay",
                    "simulation::packet::property::generated_packet_properties",
                    "--nocapture",
                ])
                .env("REPROTO_NOISE_SIM_REPORT_DIR", &directory)
                .env(
                    "REPROTO_NOISE_PROPERTY_REPORT",
                    directory.join("properties.json"),
                )
                .env("REPROTO_NOISE_PROPERTY_CASES", "16")
                .env_remove("REPROTO_NOISE_SIM_REPLAY")
                .env_remove("REPROTO_NOISE_PROPERTY_REPLAY"),
            &log(&format!("noise/simulation-{pass}")),
            0,
        )
        .unwrap();
        assert!(output.contains("Noise packet simulation: 16 schedules replayed"));
        assert!(output.contains("Noise lifecycle simulation: 16 schedules replayed"));
        assert!(output.contains("Noise packet properties: 128 generated cases replayed"));
        let reports: std::collections::BTreeMap<_, _> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), fs::read(entry.path()).unwrap())
            })
            .collect();
        assert_eq!(reports.len(), 33);
        let properties: Value =
            serde_json::from_slice(&reports[std::ffi::OsStr::new("properties.json")]).unwrap();
        let cases = properties.as_array().unwrap();
        assert_eq!(cases.len(), 128);
        for cc in ["cubic", "bbr2_gcongestion"] {
            for psk in [false, true] {
                for authenticated in [false, true] {
                    let group: Vec<_> = cases
                        .iter()
                        .filter(|c| {
                            c["case"]["cc"] == cc
                                && c["case"]["psk"] == psk
                                && c["case"]["after_handshake"] == authenticated
                        })
                        .collect();
                    assert_eq!(group.len(), 16);
                    for treatment in 0..5 {
                        assert!(group.iter().any(|c| c["treatments"][treatment].as_u64().unwrap() > 0),
                            "untested packet treatment {treatment}: {cc}, psk={psk}, authenticated={authenticated}");
                    }
                }
            }
        }
        if let Some(previous) = &previous {
            for (case, bytes) in &reports {
                assert!(
                    previous.get(case) == Some(bytes),
                    "separate processes diverged for {case:?}; inspect {}",
                    destination.display()
                );
            }
        }
        previous = Some(reports);
    }
    let provenance: Value = serde_json::from_str(
        &fs::read_to_string(root().join("vendor/provenance/quiche-revision.json")).unwrap(),
    )
    .unwrap();
    let toolchain = run(
        command("rustc").args(["--version", "--verbose"]),
        &log("noise/simulation-toolchain"),
        0,
    )
    .unwrap();
    let mut previous = previous.unwrap();
    let properties: Value = serde_json::from_slice(
        &previous
            .remove(std::ffi::OsStr::new("properties.json"))
            .unwrap(),
    )
    .unwrap();
    // Exercise the documented disk replay entry point, including rejection of
    // unsupported input formats. A seed alone is not a persisted regression.
    let replay = destination.join("property-replay.json");
    let mut input = properties[0]["case"].clone();
    for (valid, expected_exit) in [(true, 0), (false, 101)] {
        if !valid {
            input["format"] = 0.into();
        }
        fs::write(&replay, serde_json::to_vec_pretty(&input).unwrap()).unwrap();
        let output = run(
            command("cargo")
                .args([
                    "test",
                    "--locked",
                    "--manifest-path",
                    "vendor/quiche/Cargo.toml",
                    "-p",
                    "quiche",
                    "--no-default-features",
                    "--features",
                    "noise",
                    "--lib",
                    "simulation::packet::property::generated_packet_properties",
                    "--",
                    "--exact",
                    "--nocapture",
                ])
                .env("REPROTO_NOISE_PROPERTY_REPLAY", &replay)
                .env_remove("REPROTO_NOISE_SIM_REPORT_DIR")
                .env_remove("REPROTO_NOISE_PROPERTY_REPORT"),
            &log(&format!("noise/property-replay-{valid}")),
            expected_exit,
        )
        .unwrap();
        assert!(output.contains(if valid {
            "1 passed; 0 failed"
        } else {
            "unsupported packet property format"
        }));
    }
    fs::write(
        &replay,
        serde_json::to_vec_pretty(&properties[0]["case"]).unwrap(),
    )
    .unwrap();
    let reports: Vec<Value> = previous
        .into_values()
        .map(|bytes| serde_json::from_slice(&bytes).unwrap())
        .collect();
    fs::write(destination.join("checked.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "format": 3, "quiche": provenance, "toolchain": toolchain, "executable_sha256": binary_sha256,
        "features": ["noise"], "processes": 2, "schedules": reports, "properties": properties,
        "scope": "two peers, IK/IKpsk2, Cubic/BBR2; packet faults, one alternate path, bounded partition, endpoint replacement, reused CIDs and stale ciphertext; one bidirectional stream per generation; no runtime executor or OS sockets"
    })).unwrap()).unwrap();
    eprintln!("Noise simulation: 32 fixed schedules and 128 generated cases matched across two processes, including ciphertext and timers");
}

#[test]
fn quiche_noise_recovery_model() {
    let model = "verification/NoiseHandshakeRecovery.tla";
    let config = include_str!("../verification/NoiseHandshakeRecovery.cfg");
    let traces = v::exploration::traces(model, "noise-recovery", config).unwrap();
    let live = format!(
        "{}\nPROPERTY EventuallyConfirmed\n",
        config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec")
    );
    v::exploration::controls(
        model,
        "noise-recovery",
        config,
        &[
            ("initialAuthenticates", "DeliveredAuthentication"),
            ("replyAuthenticates", "DeliveredAuthentication"),
            ("dropReplyKeys", "RetainForRecovery"),
            ("retryVerifies", "ConfirmedAddress"),
        ],
        Some(&live),
    )
    .unwrap();
    let fields = ["event", "client", "server", "verified", "serverKeys"];
    let replay = traces
        .iter()
        .map(|trace| {
            trace
                .iter()
                .map(|state| {
                    fields
                        .iter()
                        .map(|field| state[*field].to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .collect::<Vec<_>>()
                .join(";")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let path = root().join("target/verification/noise-recovery/traces.txt");
    fs::write(&path, replay).unwrap();
    let output = run(
        command("cargo")
            .args([
                "test",
                "--locked",
                "--manifest-path",
                "vendor/quiche/Cargo.toml",
                "-p",
                "quiche",
                "--no-default-features",
                "--features",
                "noise",
                "--lib",
                "simulation::packet::handshake_recovery_trace_replay",
                "--",
                "--exact",
                "--nocapture",
            ])
            .env("REPROTO_NOISE_RECOVERY_TRACES", &path),
        &log("noise/recovery-replay"),
        0,
    )
    .unwrap();
    assert!(output.contains(&format!(
        "Noise recovery replay: {} traces",
        traces.len() * 4
    )));
    assert!(output.contains("1 passed; 0 failed"));
}

#[test]
fn quiche_noise_lifecycle_model() {
    let model = "verification/NoisePathLifecycle.tla";
    let config = include_str!("../verification/NoisePathLifecycle.cfg");
    let traces = v::exploration::traces(model, "noise-lifecycle", config).unwrap();
    let live = format!(
        "{}\nPROPERTY EventuallyTransferred\n",
        config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec")
    );
    v::exploration::controls(
        model,
        "noise-lifecycle",
        config,
        &[
            ("timeoutValidates", "ValidationProof"),
            ("forgedValidates", "ValidationProof"),
            ("missingMigration", "ActiveRoute"),
            ("restartKeepsAuth", "AuthenticationState"),
            ("restartCarriesStream", "StreamGeneration"),
            ("staleDelivers", "StreamGeneration"),
        ],
        Some(&live),
    )
    .unwrap();
    let fields = [
        "event",
        "client",
        "server",
        "clientGen",
        "serverGen",
        "validated",
        "active",
        "clientData",
        "serverData",
        "blocked",
        "oldBlocked",
    ];
    let replay = traces
        .iter()
        .map(|trace| {
            trace
                .iter()
                .map(|state| {
                    fields
                        .iter()
                        .map(|field| state[*field].to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .collect::<Vec<_>>()
                .join(";")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let path = root().join("target/verification/noise-lifecycle/traces.txt");
    fs::write(&path, replay).unwrap();
    let output = run(
        command("cargo")
            .args([
                "test",
                "--locked",
                "--manifest-path",
                "vendor/quiche/Cargo.toml",
                "-p",
                "quiche",
                "--no-default-features",
                "--features",
                "noise",
                "--lib",
                "simulation::packet::lifecycle::lifecycle_trace_replay",
                "--",
                "--exact",
                "--nocapture",
            ])
            .env("REPROTO_NOISE_LIFECYCLE_TRACES", &path),
        &log("noise/lifecycle-replay"),
        0,
    )
    .unwrap();
    assert!(output.contains(&format!(
        "Noise lifecycle replay: {} traces",
        traces.len() * 4
    )));
    assert!(output.contains("1 passed; 0 failed"));
}

#[test]
fn nightly_rpc_try_contracts() {
    let nightly = Some("+nightly-2026-08-29");
    for (mode, extra) in [
        ("std", &[][..]),
        (
            "alloc",
            &["--no-default-features", "--features", "alloc"][..],
        ),
    ] {
        let common = [
            "--locked",
            "--manifest-path",
            "vendor/capnp/Cargo.toml",
            "--features",
            "rpc_try",
        ];
        for (selection, expected) in [
            (&["--test", "rpc_try"][..], "8 passed; 0 failed"),
            (&["--doc", "capability::Promise"][..], "2 passed; 0 failed"),
        ] {
            let mut args = vec!["test"];
            args.extend(common);
            args.extend(extra);
            args.extend(selection);
            let output = cargo_with_toolchain(
                nightly,
                &args,
                &format!("rpc-try/{mode}-{}", selection[0].trim_start_matches('-')),
            );
            assert!(output.contains(expected), "{output}");
        }
    }
    cargo_with_toolchain(
        nightly,
        &[
            "check",
            "--locked",
            "--manifest-path",
            "vendor/capnp/Cargo.toml",
            "--no-default-features",
            "--features",
            "rpc_try",
        ],
        "rpc-try/no-alloc",
    );
    cargo_with_toolchain(
        nightly,
        &[
            "clippy",
            "--locked",
            "--manifest-path",
            "vendor/capnp/Cargo.toml",
            "--features",
            "rpc_try",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        "rpc-try/clippy",
    );
}

#[test]
fn capnp_runtime_lints() {
    // The runtime is outside the workspace: workspace --no-deps Clippy does
    // not check it as a primary package, even when its consumers are checked.
    cargo(
        &[
            "clippy",
            "--locked",
            "--manifest-path",
            "vendor/capnp/Cargo.toml",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        "capnp/clippy",
    );
}

#[test]
fn standalone_crate_tests() {
    for name in ["capnp", "capnpc", "capnp-futures"] {
        cargo(
            &[
                "test",
                "--locked",
                "--manifest-path",
                &format!("vendor/{name}/Cargo.toml"),
                "--all-targets",
            ],
            &format!("{name}/tests"),
        );
        cargo(
            &[
                "test",
                "--locked",
                "--manifest-path",
                &format!("vendor/{name}/Cargo.toml"),
                "--doc",
            ],
            &format!("{name}/doctests"),
        );
    }
    cargo(
        &[
            "check",
            "--locked",
            "--manifest-path",
            "vendor/capnp/Cargo.toml",
            "--no-default-features",
            "--features",
            "alloc",
        ],
        "capnp/alloc",
    );
    cargo(
        &[
            "check",
            "--locked",
            "--manifest-path",
            "vendor/capnp/Cargo.toml",
            "--no-default-features",
        ],
        "capnp/no-alloc",
    );
}

#[test]
fn optimized_runtime() {
    cargo(
        &[
            "test",
            "--locked",
            "--release",
            "--test",
            "storage",
            "--test",
            "durable_bulk",
            "--test",
            "persistence",
            "--test",
            "field_api_rpc",
            "--test",
            "noise",
        ],
        "optimized",
    );
}

#[test]
fn storage_crashes_in_isolated_process() {
    let output = cargo(
        &[
            "test",
            "--locked",
            "--lib",
            "storage::fault_tests",
            "--",
            "--test-threads=1",
            "--include-ignored",
        ],
        "storage-faults",
    );
    assert!(output.contains(
        "child_process_crashes_preserve_batch_atomicity_across_recovery_and_second_crash ... ok"
    ));
}

#[test]
fn eae_feasibility_probe() {
    let archive = root().join("research/EAE-Reconstruction.zip");
    assert_eq!(
        v::sha256(fs::read(&archive).unwrap()),
        "55872653058ee56813a3417df242760b81dc002e53500ca13c7c696335cf7f5b"
    );
    let destination = root().join("target/eae-benchmark");
    fs::create_dir_all(&destination).unwrap();
    run(
        command("unzip")
            .args(["-q", "-o"])
            .arg(&archive)
            .arg("-d")
            .arg(&destination),
        &log("eae/extract"),
        0,
    )
    .unwrap();
    for release in [false, true] {
        let mut cmd = command("cargo");
        cmd.args([
            "test",
            "--locked",
            "--manifest-path",
            "benchmarks/storage/Cargo.toml",
            "--test",
            "integration",
        ]);
        if release {
            cmd.arg("--release");
        }
        cmd.env("CARGO_TARGET_DIR", root().join("target/storage-benchmark"));
        let output = run(&mut cmd, &log(&format!("eae/{release}")), 0).unwrap();
        assert!(output.contains("3 passed; 0 failed"));
    }
}

#[test]
fn orphan_memory_safety() {
    let targets = [
        "orphan_external",
        "orphan_groups",
        "orphan_arena",
        "field_group_staging",
        "loaded_orphans",
        "field_owners",
    ];
    let mut args = vec!["test", "--locked", "--no-run", "--message-format=json"];
    for name in &targets {
        args.extend(["--test", name]);
    }
    let output = cargo(&args, "memcheck/build");
    let artifacts: std::collections::BTreeMap<_, _> = output
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["reason"] == "compiler-artifact" && v["executable"].is_string())
        .map(|v| {
            (
                v["target"]["name"].as_str().unwrap().to_owned(),
                v["executable"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    for name in targets {
        let output = run(
            command("valgrind").args([
                "--leak-check=full",
                "--show-leak-kinds=definite,indirect",
                "--errors-for-leak-kinds=definite,indirect",
                "--error-exitcode=99",
                &artifacts[name],
                "--skip",
                "replay_tlc_",
            ]),
            &log(&format!("memcheck/{name}")),
            0,
        )
        .unwrap();
        assert!(output.contains("ERROR SUMMARY: 0 errors"), "{output}");
        assert!(
            !output.contains("0 passed; 0 failed"),
            "empty memory test {name}"
        );
    }
}

#[test]
fn external_consumer_default_features() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path();
    fs::create_dir(app.join("src")).unwrap();
    for name in ["build.rs", "example.capnp", "src/main.rs"] {
        if root().join("examples/downstream").join(name).exists() {
            fs::copy(
                root().join("examples/downstream").join(name),
                app.join(name),
            )
            .unwrap();
        }
    }
    // Copy every schema instead of assuming its filename.
    for entry in fs::read_dir(root().join("examples/downstream")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "capnp") {
            fs::copy(&path, app.join(path.file_name().unwrap())).unwrap();
        }
    }
    let mut manifest = fs::read_to_string(root().join("examples/downstream/Cargo.toml")).unwrap();
    for suffix in ["/vendor/capnp-rpc", "/vendor/capnpc", "/vendor/capnp", ""] {
        manifest = manifest.replace(
            &format!("path = \"../..{suffix}\""),
            &format!("path = {:?}", format!("{}{suffix}", root().display())),
        );
    }
    assert!(!manifest.contains("[patch."));
    fs::write(app.join("Cargo.toml"), manifest).unwrap();
    fs::copy(root().join("Cargo.lock"), app.join("Cargo.lock")).unwrap();
    let run_consumer = |args: &[&str], name: &str| {
        run(
            command("cargo")
                .args(args)
                .arg("--manifest-path")
                .arg(app.join("Cargo.toml"))
                .current_dir(app)
                .env("CARGO_TARGET_DIR", root().join("target/downstream-check")),
            &log(&format!("downstream/{name}")),
            0,
        )
        .unwrap()
    };
    run_consumer(&["check", "--offline"], "resolve");
    run_consumer(&["run", "--locked", "--offline"], "default");
    run_consumer(&["run", "--release", "--locked", "--offline"], "optimized");
    let output = run_consumer(
        &["metadata", "--locked", "--offline", "--format-version", "1"],
        "metadata",
    );
    let metadata: Value = serde_json::from_str(output.trim()).unwrap();
    for name in ["capnp", "capnpc", "capnp-rpc", "capnp-futures"] {
        let packages: Vec<_> = metadata["packages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["name"] == name)
            .collect();
        assert_eq!(packages.len(), 1, "{name}");
        assert_eq!(
            PathBuf::from(packages[0]["manifest_path"].as_str().unwrap())
                .parent()
                .unwrap(),
            root().join("vendor").join(name)
        );
    }
}

#[test]
fn rust_guard_graph_equals_tlc() {
    v::verify(
        v::catalog()
            .groups
            .iter()
            .find(|g| g.id == "guard_model")
            .unwrap(),
    )
    .unwrap();
    let rust: Value = serde_json::from_str(
        &run(
            &mut command(env!("CARGO_BIN_EXE_reproto-model")),
            &log("guard/rust"),
            0,
        )
        .unwrap(),
    )
    .unwrap();
    let fields = [
        "head",
        "published",
        "phase",
        "pending",
        "drained",
        "direct",
        "accepted",
        "revoked",
    ];
    let source =
        fs::read_to_string(root().join("target/verification/guard/ReProto/graph.dot")).unwrap();
    let mut nodes = std::collections::BTreeMap::new();
    let mut edges = Vec::new();
    for line in source.lines() {
        let Some((id, rest)) = line.split_once(' ') else {
            continue;
        };
        if id.parse::<i64>().is_err() {
            continue;
        }
        if let Some(to) = rest.strip_prefix("-> ") {
            edges.push((id, to.split_whitespace().next().unwrap()));
        } else if let Some(label) = rest.strip_prefix("[label=\"") {
            let label = label.split('"').next().unwrap();
            let props: std::collections::BTreeMap<_, _> = label
                .split("\\n")
                .map(|part| {
                    let (name, value) = part
                        .trim()
                        .trim_start_matches(|c: char| c == '/' || c == '\\' || c.is_whitespace())
                        .split_once(" = ")
                        .unwrap();
                    (
                        name,
                        match value {
                            "TRUE" => 1,
                            "FALSE" => 0,
                            _ => value.parse::<u64>().unwrap(),
                        },
                    )
                })
                .collect();
            nodes.insert(id, fields.iter().map(|f| props[f]).collect::<Vec<_>>());
        }
    }
    let rust_states: Vec<Vec<u64>> = serde_json::from_value(rust["states"].clone()).unwrap();
    let rust_edges: Vec<(usize, usize)> = serde_json::from_value(rust["edges"].clone()).unwrap();
    assert_eq!(
        rust_states.iter().cloned().collect::<BTreeSet<_>>(),
        nodes.values().cloned().collect()
    );
    assert_eq!(
        rust_edges
            .iter()
            .map(|(a, b)| (&rust_states[*a], &rust_states[*b]))
            .collect::<BTreeSet<_>>(),
        edges.iter().map(|(a, b)| (&nodes[a], &nodes[b])).collect()
    );
}

#[test]
fn schema_pin_and_wire_inventory() {
    let pin: Value =
        serde_json::from_slice(&fs::read(root().join("vendor/provenance/revision.json")).unwrap())
            .unwrap();
    for (name, hash) in pin["schemas"].as_object().unwrap() {
        assert_eq!(
            v::sha256(fs::read(root().join("vendor/provenance").join(name)).unwrap()),
            hash.as_str().unwrap(),
            "{name}"
        );
    }
    for name in ["rpc", "rpc-twoparty", "persistent"] {
        assert_eq!(
            fs::read(root().join(format!("vendor/capnp-rpc/schema/{name}.capnp"))).unwrap(),
            fs::read(root().join(format!("vendor/capnproto/c++/src/capnp/{name}.capnp"))).unwrap()
        );
    }
    fn block<'a>(text: &'a str, declaration: &str) -> &'a str {
        let start = text
            .find(declaration)
            .unwrap_or_else(|| panic!("missing {declaration}"));
        let start = start + text[start..].find('{').unwrap() + 1;
        let mut depth = 1;
        for (offset, c) in text[start..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                return &text[start..start + offset];
            }
        }
        panic!("unclosed {declaration}")
    }
    let source = fs::read_to_string(root().join("vendor/provenance/rpc.capnp")).unwrap();
    let source = source
        .lines()
        .map(|l| l.split('#').next().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let model = fs::read_to_string(root().join("verification/CapnpWire.tla")).unwrap();
    for (name, body) in [
        ("MessageKinds", block(&source, "struct Message")),
        (
            "ReturnKinds",
            block(block(&source, "struct Return"), "union"),
        ),
        (
            "DescriptorKinds",
            block(block(&source, "struct CapDescriptor"), "union"),
        ),
        ("TargetKinds", block(&source, "struct MessageTarget")),
        (
            "DisembargoKinds",
            block(block(&source, "struct Disembargo"), "context"),
        ),
        (
            "ResultDestinations",
            block(block(&source, "struct Call"), "sendResultsTo"),
        ),
        (
            "ExceptionKinds",
            block(block(&source, "struct Exception"), "enum Type"),
        ),
        (
            "TransformKinds",
            block(block(&source, "struct PromisedAnswer"), "struct Op"),
        ),
    ] {
        let expected: BTreeSet<_> = body
            .lines()
            .filter_map(|l| l.trim().split_once('@').map(|(n, _)| n.trim()))
            .collect();
        let actual: BTreeSet<_> = block(&model, name)
            .split(',')
            .map(|n| n.trim().trim_matches('"'))
            .collect();
        assert_eq!(actual, expected, "{name}");
    }
}

#[test]
fn cpp_decodes_durable_payloads() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("objects.rp");
    for revision in [1, 2] {
        let output = run(
            command("cargo")
                .args([
                    "run",
                    "--locked",
                    "--quiet",
                    "--example",
                    "noise_store",
                    "--",
                ])
                .arg(&path),
            &log(&format!("cpp-store/{revision}")),
            0,
        )
        .unwrap();
        assert!(
            output.contains(&format!("revision {revision}:")),
            "{output}"
        );
    }
    let raw = fs::read(&path).unwrap();
    let mut offset = 64;
    let mut decoded = 0;
    let word = |pos| u64::from_le_bytes(raw[pos..pos + 8].try_into().unwrap());
    while offset < raw.len() {
        let kind = word(offset + 8);
        let length = word(offset + 32) as usize;
        if kind == 1 {
            let payload = tmp.path().join("payload.bin");
            fs::write(&payload, &raw[offset + 88..offset + 80 + length]).unwrap();
            let output = run(
                command("capnp")
                    .args(["decode", "schemas/store.capnp", "Document"])
                    .stdin(fs::File::open(payload).unwrap()),
                &log(&format!("cpp-store/decode-{decoded}")),
                0,
            )
            .unwrap();
            assert!(output.contains("Hello from Noise-backed"));
            decoded += 1;
        }
        offset += 80 + ((length + 7) & !7) + 16;
    }
    assert_eq!(decoded, 2);
    assert_eq!(offset, raw.len());
}

#[test]
fn formatting_and_lints() {
    // Do not use `fmt --all`: quiche deliberately uses nightly-only formatting.
    cargo(
        &[
            "fmt",
            "-p",
            "reproto",
            "-p",
            "reproto-test-support",
            "-p",
            "reproto-quality",
            "-p",
            "capnp-compiler",
            "-p",
            "capnp-rpc",
            "--",
            "--check",
        ],
        "quality/workspace-format",
    );
    for name in ["capnp", "capnpc", "capnp-futures"] {
        cargo(
            &[
                "fmt",
                "--manifest-path",
                &format!("vendor/{name}/Cargo.toml"),
                "--",
                "--check",
            ],
            &format!("quality/{name}-format"),
        );
    }
    cargo(
        &[
            "fmt",
            "--manifest-path",
            "benchmarks/rpc/Cargo.toml",
            "--",
            "--check",
        ],
        "quality/benchmark-format",
    );
    cargo(
        &[
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--no-deps",
            "--",
            "-D",
            "warnings",
        ],
        "quality/clippy",
    );
}

#[test]
fn fuzz_and_benchmark_harness_unit_tests() {
    for (name, manifest) in [
        ("fuzz-native", "fuzz/Cargo.toml"),
        ("benchmark-harness", "benchmarks/rpc/Cargo.toml"),
    ] {
        cargo(
            &[
                "test",
                "--locked",
                "--manifest-path",
                manifest,
                "--no-default-features",
                "--lib",
            ],
            name,
        );
    }
}
