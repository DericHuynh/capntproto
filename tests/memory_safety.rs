//! Bounded Miri qualification, invoked and checked by ordinary Cargo tests.
use capntproto_test_support::verification::{self as v, command, root, run};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const NIGHTLY: &str = "+nightly-2026-08-29";
const MANIFEST: &str = "verification/miri/Cargo.toml";
const TARGET: &str = "x86_64-unknown-linux-gnu";
const SUITES: &[(&str, usize)] = &[
    ("ownership", 9),
    ("wire", 6),
    ("orphan_types", 4),
    ("generated", 2),
    ("schema_loading", 2),
];

fn passed(output: &str) -> BTreeSet<&str> {
    let mut counts = output
        .lines()
        .filter_map(|line| {
            line.strip_prefix("test result: ok. ")?
                .split_once(" passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;")?
                .0
                .parse::<usize>()
                .ok()
        })
        .collect::<Vec<_>>();
    let mut expected: Vec<_> = SUITES.iter().map(|(_, count)| *count).collect();
    counts.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        counts, expected,
        "suite count or unignored test inventory changed"
    );
    let tests: BTreeSet<_> = output
        .lines()
        .filter_map(|line| line.strip_prefix("test ")?.strip_suffix(" ... ok"))
        .collect();
    assert_eq!(
        tests.len(),
        expected.iter().sum::<usize>(),
        "selected memory test inventory changed"
    );
    tests
}

fn source_inputs() -> BTreeMap<PathBuf, String> {
    v::distribution::sources(&root())
        .unwrap()
        .into_iter()
        .filter(|p| {
            [
                "verification/miri",
                "vendor/capnp",
                "vendor/capnp-rpc",
                "vendor/capnp-futures",
                "vendor/capnpc",
            ]
            .iter()
            .any(|prefix| p.starts_with(prefix))
                || [
                    "schemas/field-api.capnp",
                    "tests/memory_safety.rs",
                    ".cargo/config.toml",
                    "rust-toolchain.toml",
                ]
                .iter()
                .any(|file| p == Path::new(file))
        })
        .map(|p| {
            let hash = v::sha256(fs::read(root().join(&p)).unwrap());
            (p, hash)
        })
        .collect()
}

#[test]
fn serialization_and_ownership_miri() {
    let base = root().join("target/verification/memory-safety");
    fs::create_dir_all(&base).unwrap();
    let checked = base.join("checked.json");
    if checked.exists() {
        fs::remove_file(&checked).unwrap();
    }
    let extended = match std::env::var("CAPNTPROTO_MIRI_EXTENDED").as_deref() {
        Ok("1") => true,
        Err(std::env::VarError::NotPresent) | Ok("0") => false,
        _ => panic!("CAPNTPROTO_MIRI_EXTENDED must be 0 or 1"),
    };
    let directory = tempfile::tempdir_in(&base).unwrap().keep();
    let log = |name: &str| directory.join(format!("{name}.log"));
    let inputs = source_inputs();
    let compiler = run(
        command("rustc").args([NIGHTLY, "--version", "--verbose"]),
        &log("compiler"),
        0,
    )
    .unwrap();
    let interpreter = run(
        command("cargo").args([NIGHTLY, "miri", "--version"]),
        &log("miri-version"),
        0,
    )
    .unwrap();
    assert!(interpreter.starts_with("miri "));
    let schema_compiler = run(
        command("capnp").arg("--version"),
        &log("schema-compiler"),
        0,
    )
    .unwrap();
    run(
        command("cargo").args(["fmt", "--manifest-path", MANIFEST, "--", "--check"]),
        &log("format"),
        0,
    )
    .unwrap();
    run(
        command("cargo").args([
            "clippy",
            "--locked",
            "--manifest-path",
            MANIFEST,
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ]),
        &log("clippy"),
        0,
    )
    .unwrap();

    let mut evidence = Vec::new();
    for unaligned in [false, true] {
        let feature = if unaligned { "unaligned" } else { "alloc" };
        let mut selection = vec![
            "--locked",
            "--manifest-path",
            MANIFEST,
            "--features",
            feature,
        ];
        for (suite, _) in SUITES {
            selection.extend(["--test", suite]);
        }
        let native = run(
            command("cargo").arg("test").args(&selection),
            &log(&format!("native-{feature}")),
            0,
        )
        .unwrap();
        let expected = passed(&native);
        for (model, seed, extra) in [("stacked", 0, ""), ("tree", 1, " -Zmiri-tree-borrows")] {
            let flags = format!("-Zmiri-strict-provenance -Zmiri-seed={seed}{extra}");
            let output = run(
                command("cargo")
                    .args([NIGHTLY, "miri", "test"])
                    .args(&selection)
                    .args(["--target", TARGET])
                    // Do not inherit flags which disable UB checks or host isolation.
                    .env("MIRIFLAGS", &flags)
                    .env_remove("RUSTFLAGS")
                    .env_remove("CARGO_ENCODED_RUSTFLAGS")
                    .env_remove("MIRI_SYSROOT")
                    .env_remove("MIRI_LIB_SRC")
                    .env_remove("MIRI_NO_STD"),
                &log(&format!("miri-{feature}-{model}")),
                0,
            )
            .unwrap();
            assert_eq!(
                passed(&output),
                expected,
                "Miri and native inventories differ"
            );
            evidence.push(json!({"feature": feature, "aliasing": model, "seed": seed, "flags": flags, "tests": expected}));
        }
    }
    let mut extra_runs = Vec::new();
    if extended {
        // Keep the sweep bounded and independent of the existing ownership gate.
        // Wire tests are portable: no host networking, mmap, or C FFI.
        let mut inventory = None;
        for target in [TARGET, "s390x-unknown-linux-gnu"] {
            for seed in 2..6 {
                let flags = format!("-Zmiri-strict-provenance -Zmiri-seed={seed}");
                let output = run(
                    command("cargo")
                        .args([
                            NIGHTLY,
                            "miri",
                            "test",
                            "--locked",
                            "--manifest-path",
                            MANIFEST,
                            "--test",
                            "wire",
                            "--target",
                            target,
                        ])
                        .env("MIRIFLAGS", &flags)
                        .env_remove("RUSTFLAGS")
                        .env_remove("CARGO_ENCODED_RUSTFLAGS")
                        .env_remove("MIRI_SYSROOT")
                        .env_remove("MIRI_LIB_SRC")
                        .env_remove("MIRI_NO_STD"),
                    &log(&format!("miri-wire-{target}-seed-{seed}")),
                    0,
                )
                .unwrap();
                assert!(output.contains(
                    "test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"
                ));
                let names: BTreeSet<String> = output
                    .lines()
                    .filter_map(|line| {
                        line.strip_prefix("test ")?
                            .strip_suffix(" ... ok")
                            .map(str::to_owned)
                    })
                    .collect();
                assert_eq!(names.len(), 6);
                if let Some(expected) = &inventory {
                    assert_eq!(&names, expected);
                } else {
                    inventory = Some(names.clone());
                }
                extra_runs.push(json!({"target":target,"seed":seed,"flags":flags,"tests":names}));
            }
        }
    }
    let control = run(
        command("cargo")
            .args([
                NIGHTLY,
                "miri",
                "test",
                "--locked",
                "--manifest-path",
                MANIFEST,
                "--test",
                "detector",
                "--target",
                TARGET,
                "--",
                "--ignored",
                "--exact",
                "dangling_read_is_rejected",
            ])
            .env("MIRIFLAGS", "-Zmiri-strict-provenance -Zmiri-seed=0")
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("MIRI_SYSROOT")
            .env_remove("MIRI_LIB_SRC")
            .env_remove("MIRI_NO_STD"),
        &log("detector-negative-control"),
        1,
    )
    .unwrap();
    assert!(control.contains("Undefined Behavior"));
    assert!(
        control.contains("has been freed"),
        "unexpected detector failure: {control}"
    );

    assert_eq!(
        source_inputs(),
        inputs,
        "memory-check sources changed during qualification"
    );
    for file in [
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "src/lib.rs",
        "tests/detector.rs",
    ] {
        assert!(inputs.contains_key(&std::path::Path::new("verification/miri").join(file)));
    }
    for (suite, _) in SUITES {
        assert!(
            inputs.contains_key(&Path::new("verification/miri/tests").join(format!("{suite}.rs")))
        );
    }
    assert!(!inputs
        .keys()
        .any(|p| p.starts_with("verification/miri/target")));
    let tests: usize = SUITES.iter().map(|(_, count)| *count).sum();
    let report = json!({
        "format": 3, "compiler": compiler, "interpreter": interpreter, "schema_compiler": schema_compiler,
        "target": TARGET, "suites": SUITES, "runs": evidence, "native_executions": tests * 2,
        "miri_executions": tests * 4 + extra_runs.len() * 6, "extended_wire_runs": extra_runs,
        "negative_control": "freed-pointer read rejected", "inputs": inputs, "artifacts": directory,
        "scope": "23 selected tests, aligned/unaligned; checked readers, generated fields/groups/list upgrades, local capability ownership, orphan scalar/pointer/struct lists and blobs, schema ownership/atomic replacement/native registration, external immutable heap data, scratch reuse, malformed wire pointers",
        "exclusions": ["mmap and concurrent external mutation", "descriptor/OS/FFI paths", "network/RPC scheduling", "crypto", "platform behavior beyond the recorded interpreted targets", "exhaustive inputs or soundness proof"],
    });
    let bytes = serde_json::to_vec_pretty(&report).unwrap();
    fs::write(directory.join("checked.json"), &bytes).unwrap();
    fs::write(base.join("checked.json"), bytes).unwrap();
    eprintln!(
        "Miri: {tests} tests x 2 features x 2 alias models; native checks and detector control passed"
    );
}
