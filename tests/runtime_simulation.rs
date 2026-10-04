//! Qualify semantic replay of the real transport driver through private packet
//! IO. Ciphertext and internal Tokio select order are deliberately not compared.
use capntproto_test_support::verification::{self as v, command, root, run};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

const CASE: &str = "transport::simulation::tests::generated_runtime_packet_schedules";

#[test]
fn runtime_packet_scenarios_replay_across_processes() {
    let base = root().join("target/verification/runtime-socket-qualification");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap().keep();
    let inputs = v::distribution::verification_hashes(&root()).unwrap();
    let compiler = run(
        command("rustc").args(["--version", "--verbose"]),
        &directory.join("compiler.log"),
        0,
    )
    .unwrap();
    let metadata = directory.join("binaries.json");
    let build = v::run_stdout(
        command("cargo").args([
            "nextest",
            "list",
            "--locked",
            "--no-default-features",
            "--features",
            "native",
            "--lib",
            "--list-type=binaries-only",
            "--message-format=json",
        ]),
        &directory.join("build.log"),
        0,
    )
    .unwrap();
    fs::write(&metadata, &build).unwrap();
    let inventory: Value = serde_json::from_str(&build).unwrap();
    let binaries: Vec<PathBuf> = inventory["rust-binaries"]
        .as_object()
        .unwrap()
        .values()
        .map(|value| PathBuf::from(value["binary-path"].as_str().unwrap()))
        .collect();
    assert_eq!(binaries.len(), 1);
    let binary = &binaries[0];
    let mut cases = Vec::new();
    for process in 0..2 {
        let output = directory.join(format!("process-{process}"));
        let result = run(
            v::nextest::json_output(
                command("cargo")
                    .args(["nextest", "run", "--binaries-metadata"])
                    .arg(&metadata),
            )
            .args(["--", "--exact", CASE])
            .env("CAPNTPROTO_RUNTIME_SIM_OUTPUT", &output)
            .env_remove("CAPNTPROTO_RUNTIME_SIM_REPLAY"),
            &directory.join(format!("sample-{process}.log")),
            0,
        )
        .unwrap();
        assert_eq!(v::nextest::passed(&result).len(), 1);
        let saved: Value =
            serde_json::from_slice(&fs::read(output.join("runs.json")).unwrap()).unwrap();
        assert_eq!(saved["format"], 1);
        assert_eq!(saved["cases"].as_array().unwrap().len(), 32);
        cases.push(saved);
    }
    assert_eq!(
        cases[0], cases[1],
        "input/semantic outcome drift across processes"
    );
    let replay = directory.join("process-0/runs.json");
    let output = directory.join("replay");
    let result = run(
        v::nextest::json_output(
            command("cargo")
                .args(["nextest", "run", "--binaries-metadata"])
                .arg(&metadata),
        )
        .args(["--", "--exact", CASE])
        .env("CAPNTPROTO_RUNTIME_SIM_OUTPUT", &output)
        .env("CAPNTPROTO_RUNTIME_SIM_REPLAY", &replay),
        &directory.join("replay.log"),
        0,
    )
    .unwrap();
    assert_eq!(v::nextest::passed(&result).len(), 1);
    let replayed: Value =
        serde_json::from_slice(&fs::read(output.join("runs.json")).unwrap()).unwrap();
    assert_eq!(replayed, cases[0]);
    // Replaying into the input directory must preserve the valid run.
    run(
        v::nextest::json_output(
            command("cargo")
                .args(["nextest", "run", "--binaries-metadata"])
                .arg(&metadata),
        )
        .args(["--", "--exact", CASE])
        .env("CAPNTPROTO_RUNTIME_SIM_OUTPUT", directory.join("process-0"))
        .env("CAPNTPROTO_RUNTIME_SIM_REPLAY", &replay),
        &directory.join("in-place-replay.log"),
        0,
    )
    .unwrap();
    let in_place: Value = serde_json::from_slice(&fs::read(&replay).unwrap()).unwrap();
    assert_eq!(in_place, cases[0]);
    let mut corrupted = cases[0].clone();
    corrupted["cases"].as_array_mut().unwrap().truncate(1);
    corrupted["cases"][0][2]["receipts"][0] = json!(u64::MAX);
    let bad = directory.join("incorrect-receipt.json");
    fs::write(&bad, serde_json::to_vec_pretty(&corrupted).unwrap()).unwrap();
    let failure = run(
        v::nextest::json_output(
            command("cargo")
                .args(["nextest", "run", "--binaries-metadata"])
                .arg(&metadata),
        )
        .args(["--", "--exact", CASE])
        .env("CAPNTPROTO_RUNTIME_SIM_OUTPUT", &output)
        .env("CAPNTPROTO_RUNTIME_SIM_REPLAY", bad),
        &directory.join("control.log"),
        100,
    )
    .unwrap();
    assert!(failure.contains("runtime semantic replay diverged"));
    assert!(
        !output.join("runs.json").exists(),
        "failed replay retained stale success evidence"
    );
    assert_eq!(
        inputs,
        v::distribution::verification_hashes(&root()).unwrap()
    );
    let evidence = json!({
        "format":1,"compiler":compiler,"inputs":inputs,"binary":binary,
        "binary_sha256":v::sha256(fs::read(binary).unwrap()),
        "runs_sha256":v::sha256(fs::read(replay).unwrap()),"cases":32,"processes":2,
        "scope":"real Native mutual TLS with optional admission secrets driver; real upstream recovery clock and paused application deadlines; in-memory packet IO; generated delivery faults and semantic replay",
        "limits":"fixed two-peer paths, bounded schedules; real clock timing, crypto entropy and internal Tokio select order uncontrolled; no ciphertext or poll-order equivalence claim"
    });
    fs::write(
        directory.join("qualification.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}
