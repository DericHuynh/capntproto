//! Compiler, distribution and reference checks, all launched by `cargo nextest run`.
use capntproto_test_support::verification::{self as v, command, root, run};
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
    let native_tests = args.starts_with(&["nextest", "run"]);
    let mut cmd = command("cargo");
    if let (Ok(build), Ok(profiles), Ok(flags)) = (
        std::env::var("CAPNTPROTO_FULL_COVERAGE_BUILD"),
        std::env::var("CAPNTPROTO_FULL_COVERAGE_PROFILES"),
        std::env::var("CAPNTPROTO_FULL_COVERAGE_FLAGS"),
    ) {
        if native_tests || args.first() == Some(&"test") {
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
                    .env("CAPNTPROTO_COVERAGE_CHILDREN_NATIVE", "1");
                cmd.args(&args[..if native_tests { 2 } else { 1 }])
                    .arg("--ignore-rust-version");
                if native_tests && !args.contains(&"--no-run") {
                    v::nextest::json_output(&mut cmd);
                }
                cmd.args(&args[if native_tests { 2 } else { 1 }..]);
                return run(&mut cmd, &log(name), 0).unwrap();
            }
        }
    }
    if let Some(toolchain) = toolchain {
        cmd.arg(toolchain);
    }
    if native_tests && !args.contains(&"--no-run") {
        cmd.args(&args[..2]);
        v::nextest::json_output(&mut cmd).args(&args[2..]);
        return run(&mut cmd, &log(name), 0).unwrap();
    }
    if args.first() == Some(&"metadata") {
        // Cargo can download target-specific dependencies on a cold runner.
        // Its progress and warnings belong to stderr, never the JSON payload.
        v::run_stdout(cmd.args(args), &log(name), 0).unwrap()
    } else {
        run(cmd.args(args), &log(name), 0).unwrap()
    }
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
    let output = v::run_stdout(
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
    let tests = v::run_stdout(
        command("cargo")
            .args([
                "+1.97.0",
                "auditable",
                "nextest",
                "run",
                "--locked",
                "--offline",
                "--no-run",
                "--cargo-message-format=json",
                "--manifest-path",
            ])
            .arg(project.path().join("Cargo.toml"))
            .arg("--target-dir")
            .arg(&target),
        &log("auditable-nextest"),
        0,
    )
    .unwrap();
    let test_binary: Value = tests
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value["profile"]["test"] == true && value["executable"].is_string())
        .expect("nextest must expose the compiled test executable");
    // cargo-auditable supports nextest's build invocation, but Rust test
    // harnesses are not eligible for its embedded inventory (unlike binaries).
    assert!(std::path::Path::new(test_binary["executable"].as_str().unwrap()).is_file());
    let inventory = project.path().join("auditable.json");
    run(
        command("cargo")
            .args([
                "run",
                "--locked",
                "-p",
                "capntproto-dev",
                "--",
                "check-auditable",
                "--output",
            ])
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
        command("cargo")
            .args([
                "run",
                "--locked",
                "-p",
                "capntproto-dev",
                "--",
                "check-auditable",
                "--output",
            ])
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
    fs::write(project.path().join("Cargo.toml"), format!("[package]\nname = \"field-api-acceptance\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\ncapnp = {{ package = \"capntproto-core\", path = {:?} }}\n[build-dependencies]\ncapnpc = {{ package = \"capntproto-codegen\", path = {:?} }}\n", root().join("crates/capntproto-core"), root().join("crates/capntproto-codegen"))).unwrap();
    fs::write(project.path().join("build.rs"), format!("fn main() {{capnpc::CompilerCommand::new().src_prefix({:?}).file({:?}).import_path({:?}).field_api(true).field_api_values(true).field_api_projections(true).run().unwrap();}}",root().join("schemas"), root().join("schemas/field-api.capnp"), root().join("crates/capntproto-codegen"))).unwrap();
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
fn pinned_native_profile() {
    let graph = cargo(
        &["tree", "--locked", "-e", "normal,build"],
        "native/dependencies",
    );
    assert!(!graph.contains("snow v"));
    assert!(graph.contains("rustls v"));
    assert!(graph.contains("boring v"));
    let output = cargo(
        &["metadata", "--locked", "--format-version", "1"],
        "native/metadata",
    );
    let metadata: Value = serde_json::from_str(output.trim()).unwrap();
    let packages: Vec<_> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["name"] == "quiche")
        .collect();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0]["version"], "0.30.0");
    assert_eq!(
        packages[0]["source"],
        "registry+https://github.com/rust-lang/crates.io-index"
    );
    let node = metadata["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == packages[0]["id"])
        .unwrap();
    assert!(!node["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f == "fuzzing"));
    assert!(!root().join("vendor/quiche").exists());
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
            "crates/capntproto-core/Cargo.toml",
            "--features",
            "rpc_try",
        ];
        for (selection, expected) in [
            (&["--test", "rpc_try"][..], "8 passed; 0 failed"),
            (&["--doc", "capability::Promise"][..], "2 passed; 0 failed"),
        ] {
            let mut args = if selection[0] == "--doc" {
                vec!["test"]
            } else {
                vec!["nextest", "run"]
            };
            args.extend(common);
            args.extend(extra);
            args.extend(selection);
            let output = cargo_with_toolchain(
                nightly,
                &args,
                &format!("rpc-try/{mode}-{}", selection[0].trim_start_matches('-')),
            );
            if selection[0] == "--doc" {
                assert!(output.contains(expected), "{output}");
            } else {
                assert_eq!(v::nextest::passed(&output).len(), 8);
            }
        }
    }
    cargo_with_toolchain(
        nightly,
        &[
            "check",
            "--locked",
            "--manifest-path",
            "crates/capntproto-core/Cargo.toml",
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
            "crates/capntproto-core/Cargo.toml",
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
fn core_feature_profiles() {
    // All owned crates' ordinary tests/lints are covered by --workspace.
    cargo(
        &[
            "check",
            "--locked",
            "--manifest-path",
            "crates/capntproto-core/Cargo.toml",
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
            "crates/capntproto-core/Cargo.toml",
            "--no-default-features",
        ],
        "capnp/no-alloc",
    );
}

fn optimized_checks(filters: &[&str], name: &str) {
    let mut args = vec![
        "nextest",
        "run",
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
        "native",
        "--",
    ];
    args.extend(filters);
    cargo(&args, name);
}

#[test]
fn optimized_runtime() {
    optimized_checks(&["--skip", "tlc"], "optimized");
}

#[test]
fn tlc_optimized_runtime() {
    optimized_checks(&["tlc"], "optimized-models");
}

#[test]
fn storage_crashes_in_isolated_process() {
    let output = cargo(
        &[
            "nextest",
            "run",
            "--locked",
            "--lib",
            "--test-threads=1",
            "--",
            "storage::fault_tests",
            "storage::components::crash_tests",
            "--include-ignored",
        ],
        "storage-faults",
    );
    let passed = v::nextest::passed(&output);
    for case in [
        "child_process_crashes_preserve_batch_atomicity_across_recovery_and_second_crash",
        "child_process_crashes_preserve_components_across_recovery_and_second_crash",
    ] {
        assert!(
            passed.iter().any(|name| name.ends_with(case)),
            "missing crash recovery check: {case}"
        );
    }
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
        cmd.args(["nextest", "run"]);
        v::nextest::json_output(&mut cmd);
        cmd.args([
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
        assert_eq!(v::nextest::passed(&output).len(), 3);
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
    for name in targets {
        let output = run(
            command("cargo").args([
                "nextest", "run", "--locked", "--test", name,
                "--no-capture", "-E", "not test(replay_tlc_)",
            ]).env("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER",
                "valgrind --leak-check=full --show-leak-kinds=definite,indirect --errors-for-leak-kinds=definite,indirect --error-exitcode=99"),
            &log(&format!("memcheck/{name}")), 0,
        ).unwrap();
        assert!(output.contains("ERROR SUMMARY: 0 errors"), "{output}");
        // Nextest fails on an empty selection. Valgrind's nonzero exit fails the test.
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
    for suffix in [
        "/crates/capntproto-rpc",
        "/crates/capntproto-codegen",
        "/crates/capntproto-core",
        "",
    ] {
        manifest = manifest.replace(
            &format!("path = \"../..{suffix}\""),
            &format!("path = {:?}", format!("{}{suffix}", root().display())),
        );
    }
    assert!(!manifest.contains("[patch."));
    fs::write(app.join("Cargo.toml"), manifest).unwrap();
    fs::copy(root().join("Cargo.lock"), app.join("Cargo.lock")).unwrap();
    let run_consumer = |args: &[&str], name: &str| {
        let runner = if args.first() == Some(&"metadata") {
            v::run_stdout
        } else {
            run
        };
        runner(
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
    // Metadata resolves every target, even dependencies not downloaded by the
    // host-only build (for example r-efi). Fetch the resolved graph explicitly
    // before requiring the subsequent metadata query to work offline.
    run_consumer(&["fetch", "--locked"], "fetch-metadata-dependencies");
    let output = run_consumer(
        &["metadata", "--locked", "--offline", "--format-version", "1"],
        "metadata",
    );
    let metadata: Value = serde_json::from_str(output.trim()).unwrap();
    for name in [
        "capntproto-core",
        "capntproto-codegen",
        "capntproto-rpc",
        "capntproto-futures",
    ] {
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
            root().join("crates").join(name)
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
        &v::run_stdout(
            &mut command(env!("CARGO_BIN_EXE_capntproto-model")),
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
            fs::read(root().join(format!("crates/capntproto-rpc/schema/{name}.capnp"))).unwrap(),
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
                    "native_store",
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
            assert!(output.contains("Hello from Native-backed"));
            decoded += 1;
        }
        offset += 80 + ((length + 7) & !7) + 16;
    }
    assert_eq!(decoded, 2);
    assert_eq!(offset, raw.len());
}

#[test]
fn formatting_and_lints() {
    cargo(
        &["fmt", "--all", "--", "--check"],
        "quality/workspace-format",
    );
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
                "nextest",
                "run",
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
