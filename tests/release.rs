use capntproto_test_support::verification::{command, distribution as d, root, run_with_timeout};
use std::fs;

#[test]
fn source_bundle_roundtrip() {
    let temp = tempfile::tempdir().unwrap();
    let a = d::build(&root(), &temp.path().join("first.tar.gz"), false).unwrap();
    let b = d::build(&root(), &temp.path().join("second.tar.gz"), false).unwrap();
    assert_eq!(a.sha256, b.sha256);
    let extracted = d::unpack(
        &temp.path().join("first.tar.gz"),
        &temp.path().join("extracted"),
    )
    .unwrap();
    // Source bundles remain self-contained when the reference is a submodule.
    assert!(extracted.join(".gitmodules").is_file());
    assert!(extracted.join("vendor/capnproto/CMakeLists.txt").is_file());
    assert!(extracted
        .join("vendor/capnproto/c++/src/capnp/rpc.c++")
        .is_file());
    assert!(!extracted.join("vendor/capnproto/.git").exists());
    for report in ["storage-next", "storage-resilience", "concurrency"] {
        for file in ["environment.json", "runs.jsonl", "summary.json"] {
            assert!(extracted
                .join(format!("research/reports/{report}/2026-10-01/{file}"))
                .is_file());
        }
    }
    for file in ["environment.json", "results.json"] {
        assert!(extracted
            .join(format!("research/reports/rpc-pipeline/2026-10-01/{file}"))
            .is_file());
    }
    // Exercise source-only exclusions even in a clean CI checkout. Archive
    // validation above inventories every file; only subsequent source scans
    // exclude newly generated fuzz results.
    for name in ["fuzz/artifacts/crash", "fuzz/corpus/seed"] {
        let file = extracted.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, b"generated fuzz data").unwrap();
    }
    assert_eq!(
        d::source_hashes(&root()).unwrap(),
        d::source_hashes(&extracted).unwrap()
    );
}

#[test]
#[ignore = "full clean build; cargo nextest run --test release isolated_release_qualification -- --ignored --exact"]
fn isolated_release_qualification() {
    let before = d::verification_hashes(&root()).unwrap();
    let work = tempfile::Builder::new()
        .prefix(".capntproto-cargo-release-")
        .tempdir_in(root().parent().unwrap())
        .unwrap()
        .keep();
    let bundle = d::build(&root(), &work.join("source.tar.gz"), false).unwrap();
    let extracted = d::unpack(&work.join("source.tar.gz"), &work.join("extracted")).unwrap();
    let reports = root().join("target/release-qualification");
    fs::create_dir_all(&reports).unwrap();
    for (name, args) in [
        (
            "tests",
            vec!["nextest", "run", "--locked", "--workspace", "--all-targets"],
        ),
        ("doctests", vec!["test", "--locked", "--workspace", "--doc"]),
        (
            "clippy",
            vec![
                "clippy",
                "--locked",
                "--workspace",
                "--all-targets",
                "--no-deps",
                "--",
                "-D",
                "warnings",
            ],
        ),
    ] {
        run_with_timeout(
            command("cargo")
                .args(args)
                .current_dir(&extracted)
                .env_remove("CARGO_TARGET_DIR")
                .env("CAPNTPROTO_CHECK_TIMEOUT", "7200"),
            &reports.join(format!("cargo-{name}.log")),
            0,
            std::time::Duration::from_secs(7200),
        )
        .unwrap();
    }
    assert_eq!(
        before,
        d::verification_hashes(&root()).unwrap(),
        "sources changed during qualification"
    );
    let evidence = serde_json::json!({"sources":before,"archive_sha256":bundle.sha256,"source_tree":extracted,"scope":"isolated Linux source reconstruction, all native Cargo tests, doctests and Clippy","limits":"dependency download caches reused; no public publication or hardware power-loss proof"});
    fs::write(
        reports.join("cargo-qualification.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    d::build(
        &root(),
        &root().join("dist/capntproto-0.1.0-source.tar.gz"),
        true,
    )
    .unwrap();
}

#[test]
#[ignore = "requires current qualification; cargo nextest run --test release build_qualified_source -- --ignored --exact"]
fn build_qualified_source() {
    d::build(
        &root(),
        &root().join("dist/capntproto-0.1.0-source.tar.gz"),
        true,
    )
    .unwrap();
}
