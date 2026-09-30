//! Scoped implementation mutation and LLVM coverage evidence.
use reproto_test_support::verification::{self as v, command, root, run};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::Duration,
};

const VERSION: &str = "cargo-mutants 27.1.0";
const FILES: &[&str] = &[
    "src/authority.rs",
    "src/semantics/handoff.rs",
    "src/semantics/stream.rs",
];
const SELECTION: &[&str] = &[
    "--lib",
    "--test",
    "authority",
    "--",
    "--skip",
    "replay_tlc_",
    "--skip",
    "unix_rpc::",
];

fn native_tests(output: &str, suffix: &str) -> BTreeSet<String> {
    output
        .lines()
        .filter_map(|line| {
            let line = if suffix == " ... ok" {
                line.strip_prefix("test ")?
            } else {
                line
            };
            line.strip_suffix(suffix).map(str::to_owned)
        })
        .collect()
}

fn selected_tests(output: &str) -> BTreeSet<String> {
    let tests = native_tests(output, ": test");
    for required in [
        "exhaustive_rights_decoding_and_attenuation",
        "revocation_waits_observe_ancestors_before_and_after_registration",
        "attenuation_and_branch_revocation",
        "embargo_and_replay",
        "native_stream_gate_enforces_roles_and_terminal_preface_failure",
        "revocation_preserves_phase_and_allows_only_pending_cleanup",
        "semantics::handoff::tests::enqueue_reserves_drain_capacity_before_reporting_success",
    ] {
        assert!(
            tests.contains(required),
            "missing critical native test: {required}"
        );
    }
    tests
}

#[test]
fn authority_and_transition_mutations() {
    let base = root().join("target/verification/rust-mutations");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(&base).unwrap().keep();
    let log = |name: &str| directory.join(format!("{name}.log"));
    eprintln!("Rust mutation artifacts: {}", directory.display());
    let version = run(
        command("cargo").args(["mutants", "--version"]),
        &log("version"),
        0,
    )
    .unwrap();
    assert_eq!(
        version.trim(),
        VERSION,
        "install with cargo install cargo-mutants --version 27.1.0 --locked"
    );
    let compiler = run(
        command("rustc").args(["--version", "--verbose"]),
        &log("compiler"),
        0,
    )
    .unwrap();

    let inputs = v::distribution::source_hashes(&root()).unwrap();
    let tree = directory.join("sources");
    for path in inputs.keys() {
        let destination = tree.join(path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(root().join(path), destination).unwrap();
    }
    assert_eq!(v::distribution::source_hashes(&tree).unwrap(), inputs);
    let target = directory.join("build");
    let mut inventory = command("cargo");
    inventory
        .current_dir(&tree)
        .env("CARGO_TARGET_DIR", &target)
        .args(["test", "--locked", "--no-default-features"])
        .args(SELECTION)
        .arg("--list");
    let inventory = run(&mut inventory, &log("inventory"), 0).unwrap();
    let tests = selected_tests(&inventory);
    let mut mutate = command("cargo");
    mutate
        .current_dir(&tree)
        .env("CARGO_TARGET_DIR", &target)
        .args([
            "mutants",
            "--no-config",
            "--no-default-features",
            "--in-place",
            "--package",
            "reproto",
            "--baseline",
            "run",
            "--build-timeout",
            "180",
            "--timeout",
            "20",
            "--cap-lints",
            "true",
            "--colors",
            "never",
            "--cargo-arg=--locked",
            "--cargo-arg=--lib",
            "--cargo-arg=--test=authority",
            "--cargo-arg=--no-fail-fast",
            "--output",
        ])
        .arg(directory.join("results"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("CARGO_MUTANTS_") {
            mutate.env_remove(key);
        }
    }
    for file in FILES {
        mutate.args(["--file", file]);
    }
    mutate.arg("--").args(&SELECTION[3..]);
    let outcome = v::run_with_timeout(&mut mutate, &log("mutations"), 0, Duration::from_secs(1800));
    // Check originals even when mutants survive or a child command fails.
    assert_eq!(
        v::distribution::source_hashes(&root()).unwrap(),
        inputs,
        "working sources changed during qualification"
    );
    assert_eq!(
        v::distribution::source_hashes(&tree).unwrap(),
        inputs,
        "mutator did not restore the disposable source tree"
    );
    outcome.unwrap();

    let output = directory.join("results/mutants.out");
    let report: Value =
        serde_json::from_slice(&fs::read(output.join("outcomes.json")).unwrap()).unwrap();
    assert_eq!(report["cargo_mutants_version"], "27.1.0");
    let inventory: Vec<Value> =
        serde_json::from_slice(&fs::read(output.join("mutants.json")).unwrap()).unwrap();
    let inventory: BTreeMap<_, _> = inventory
        .into_iter()
        .map(|mut mutant| {
            mutant.as_object_mut().unwrap().remove("diff");
            (mutant["name"].as_str().unwrap().to_owned(), mutant)
        })
        .collect();
    assert!(inventory.len() >= 94, "incomplete mutation inventory");
    assert_eq!(report["total_mutants"], inventory.len());
    for field in ["missed", "timeout"] {
        assert_eq!(report[field], 0, "{field}");
    }
    assert_eq!(report["unviable"], 7);
    assert_eq!(
        report["caught"].as_u64().unwrap() + 7,
        report["total_mutants"]
    );
    let mut baseline = 0;
    let mut observed = BTreeMap::new();
    for case in report["outcomes"].as_array().unwrap() {
        let path = Path::new(case["log_path"].as_str().unwrap());
        assert!(
            path.is_relative()
                && !path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
        );
        let text = fs::read_to_string(output.join(path)).unwrap();
        let phases = case["phase_results"].as_array().unwrap();
        assert_eq!(phases[0]["phase"], "Build");
        if case["scenario"] != "Baseline" {
            let mutant = &case["scenario"]["Mutant"];
            assert!(FILES.contains(&mutant["file"].as_str().unwrap()));
            assert!(observed
                .insert(mutant["name"].as_str().unwrap().to_owned(), mutant.clone())
                .is_none());
        }
        if case["summary"] == "Unviable" {
            assert_eq!(phases.len(), 1);
            assert_eq!(phases[0]["process_status"], json!({"Failure":101}));
        } else {
            assert_eq!(phases.len(), 2);
            assert_eq!(phases[0]["process_status"], "Success");
            assert_eq!(phases[1]["phase"], "Test");
            assert_eq!(
                phases[1]["process_status"],
                if case["scenario"] == "Baseline" {
                    json!("Success")
                } else {
                    json!({"Failure":101})
                }
            );
        }
        if case["scenario"] == "Baseline" {
            baseline += 1;
            assert_eq!(case["summary"], "Success");
            assert_eq!(native_tests(&text, " ... ok"), tests);
        } else if case["summary"] == "Unviable" {
            let mutant = &case["scenario"]["Mutant"];
            let function = mutant["function"]["function_name"].as_str().unwrap();
            let (replacement, ty) = match function {
                "Rights::from_bits" => ("Some(Default::default())", "Rights"),
                "Grant::root" => ("Default::default()", "Grant"),
                "Grant::object" => ("Default::default()", "ObjectId"),
                "Grant::generation" => ("Default::default()", "ObjectGeneration"),
                "Grant::rights" => ("Default::default()", "Rights"),
                "Grant::delegate" => ("Some(Default::default())", "Grant"),
                "NativeStreamGate::state" => ("Default::default()", "NativeStreamState"),
                _ => panic!("unexpected unbuildable mutation: {mutant}"),
            };
            assert_eq!(mutant["replacement"], replacement);
            assert!(text.contains(&format!("{ty}: Default")) && text.contains("error[E0277]"));
            assert!(!text.contains("Running unittests"));
        } else {
            assert_eq!(case["summary"], "CaughtMutant");
            assert!(
                text.contains("test result: FAILED."),
                "a tool/build failure is not a caught assertion"
            );
            assert!(
                tests
                    .iter()
                    .any(|name| text.contains(&format!("test {name} ... FAILED"))),
                "failure not attributed to a selected test"
            );
        }
    }
    assert_eq!(baseline, 1);
    assert_eq!(
        observed, inventory,
        "missing, duplicated or altered mutations"
    );
    let evidence = json!({
        "format": 1, "scope": "authority and handoff/stream transition guards",
        "tool": version.trim(), "compiler": compiler, "files": FILES,
        "features": [], "tests": tests, "inputs": inputs, "results": report,
        "artifacts": directory,
        "limits": "selected implementation mutations; excludes Noise crypto, native drivers, storage, concurrency schedules and TLC mutants",
    });
    fs::write(
        base.join("checked.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}

#[test]
fn authority_and_transition_coverage() {
    let base = root().join("target/verification/guard-coverage");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(&base).unwrap().keep();
    let log = |name: &str| directory.join(format!("{name}.log"));
    let version = run(
        command("cargo").args(["llvm-cov", "--version"]),
        &log("version"),
        0,
    )
    .unwrap();
    assert_eq!(version.trim(), "cargo-llvm-cov 0.8.6",
        "install cargo-llvm-cov --version 0.8.6 --locked and rustup component add llvm-tools-preview");
    let compiler = run(
        command("rustc").args(["--version", "--verbose"]),
        &log("compiler"),
        0,
    )
    .unwrap();
    let inputs = v::distribution::source_hashes(&root()).unwrap();
    let inventory = run(
        command("cargo")
            .args(["test", "--locked", "--no-default-features"])
            .args(SELECTION)
            .arg("--list"),
        &log("inventory"),
        0,
    )
    .unwrap();
    let tests = selected_tests(&inventory);
    let export = directory.join("coverage.json");
    let target = directory.join("build");
    let output = run(
        command("cargo")
            .env("CARGO_LLVM_COV_TARGET_DIR", &target)
            .args([
                "llvm-cov",
                "--locked",
                "--no-default-features",
                "--no-cfg-coverage",
                "--json",
                "--output-path",
            ])
            .arg(&export)
            .args(SELECTION),
        &log("coverage"),
        0,
    )
    .unwrap();
    assert_eq!(native_tests(&output, " ... ok"), tests);
    run(
        command("cargo")
            .env("CARGO_LLVM_COV_TARGET_DIR", &target)
            .args(["llvm-cov", "report", "--html", "--output-dir"])
            .arg(directory.join("html")),
        &log("html"),
        0,
    )
    .unwrap();
    assert_eq!(v::distribution::source_hashes(&root()).unwrap(), inputs);
    let report: Value = serde_json::from_slice(&fs::read(&export).unwrap()).unwrap();
    assert_eq!(report["type"], "llvm.coverage.json.export");
    let mut files = BTreeMap::new();
    for file in report["data"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|data| data["files"].as_array().unwrap())
    {
        let path = Path::new(file["filename"].as_str().unwrap());
        let Ok(relative) = path.strip_prefix(root()) else {
            continue;
        };
        let Some(relative) = relative.to_str() else {
            continue;
        };
        if FILES.contains(&relative) {
            assert!(file["summary"]["regions"]["covered"].as_u64().unwrap() > 0);
            let uncovered: Vec<_> = file["segments"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s[2] == 0 && s[3] == true)
                .collect();
            assert!(files
                .insert(
                    relative.to_owned(),
                    json!({"summary":file["summary"], "uncovered_segments":uncovered})
                )
                .is_none());
        }
    }
    assert_eq!(
        files.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        FILES.iter().copied().collect()
    );
    for (name, file) in &files {
        eprintln!(
            "{name}: lines {}, regions {}",
            file["summary"]["lines"], file["summary"]["regions"]
        );
    }
    let evidence = json!({"format":1, "tool":version.trim(), "compiler":compiler,
        "features":[], "tests":tests, "inputs":inputs, "files":files,
        "raw_export_sha256":v::sha256(fs::read(export).unwrap()), "artifacts":directory,
        "limits":"line/region coverage of three production files under selected native tests; no branch/MC/DC instrumentation, TLC replay, default features, crypto, runtime or workspace coverage claim"});
    fs::write(
        base.join("checked.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}
