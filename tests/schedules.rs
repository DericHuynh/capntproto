//! Build and run the async fixtures in independent processes; qualify replay
//! and deadlock detection before publishing schedule evidence.
use capntproto_test_support::{
    schedules::{Saved, ITERATIONS, SEED},
    verification::{self as v, command, root, run},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const CASES: &[(&str, &str, &str)] = &[
    (
        "capntproto",
        "native_shutdown::schedule_tests::shuttle_shutdown_first_completion_wakes_all_waiters",
        "shutdown-completion",
    ),
    (
        "capntproto",
        "native_shutdown::schedule_tests::shuttle_shutdown_cancellation_and_generation_isolation",
        "shutdown-cancellation",
    ),
    (
        "authority_schedules",
        "shuttle_grant_waiters_cancel_replace_and_preserve_sibling_isolation",
        "grant-waiters",
    ),
    (
        "capntproto",
        "native_rpc::session::schedule_tests::shuttle_route_owners_cancel_pending_tasks_and_isolate_generations",
        "route-owners",
    ),
    (
        "capntproto",
        "native_rpc::session::output_tests::shuttle_output_fences_cancel_replace_and_preserve_order",
        "route-output-success",
    ),
    (
        "capntproto",
        "native_rpc::session::output_tests::shuttle_output_failures_race_drain_and_owner_stop",
        "route-output-errors",
    ),
    (
        "capntproto",
        "native_rpc::session::shutdown_tests::shuttle_shutdown_fences_compete_with_deadline",
        "shutdown-fences",
    ),
    (
        "capntproto",
        "native_rpc::session::shutdown_tests::shuttle_shutdown_terminal_causes_and_cancellation_wake_blocked_fences",
        "shutdown-terminal",
    ),
];

fn child(binary: &Path, test: &str, output: &Path) -> Command {
    let mut cmd = command(binary);
    cmd.args(["--exact", test, "--nocapture"])
        .env("CAPNTPROTO_SCHEDULE_OUTPUT", output)
        .env_remove("CAPNTPROTO_SCHEDULE_REPLAY");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("SHUTTLE_") {
            cmd.env_remove(name);
        }
    }
    cmd
}

fn passed(cmd: &mut Command, log: &Path) {
    let output = run(cmd, log, 0).unwrap();
    assert!(
        output.contains("test result: ok. 1 passed; 0 failed; 0 ignored;"),
        "missing scenario execution: {output}"
    );
}

#[test]
fn recorded_async_schedules_replay_across_processes() {
    let base = root().join("target/verification/async-schedules");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(&base).unwrap().keep();
    let inputs = v::distribution::verification_hashes(&root()).unwrap();
    let compiler = run(
        command("rustc").args(["--version", "--verbose"]),
        &directory.join("compiler.log"),
        0,
    )
    .unwrap();
    let build = run(
        command("cargo").args([
            "test",
            "--locked",
            "--no-default-features",
            "--features",
            "native",
            "--lib",
            "--test",
            "authority_schedules",
            "--no-run",
            "--message-format=json",
        ]),
        &directory.join("build.log"),
        0,
    )
    .unwrap();
    let binaries: BTreeMap<String, PathBuf> = build
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| value["reason"] == "compiler-artifact" && value["profile"]["test"] == true)
        .filter_map(|value| {
            Some((
                value["target"]["name"].as_str()?.to_owned(),
                PathBuf::from(value["executable"].as_str()?),
            ))
        })
        .collect();
    assert_eq!(binaries.len(), 2);
    let mut evidence = Vec::new();
    for (binary, test, name) in CASES {
        let binary = &binaries[*binary];
        let mut first: Option<Vec<u8>> = None;
        for process in 0..2 {
            let output = directory.join(format!("process-{process}"));
            passed(
                &mut child(binary, test, &output),
                &directory.join(format!("{name}-{process}.log")),
            );
            let bytes = fs::read(output.join(name).join("runs.json")).unwrap();
            let saved: Saved = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(saved.format, 1);
            assert_eq!(saved.case, *name);
            assert_eq!(saved.runs.len(), ITERATIONS);
            for run in &saved.runs {
                assert!(!run.tasks.is_empty() && run.tasks.len() <= 512);
                assert_eq!(run.events.last(), Some(&("case-complete".into(), 1)));
            }
            if let Some(first) = &first {
                assert_eq!(&bytes, first, "cross-process schedule/event drift");
            } else {
                first = Some(bytes);
            }
        }
        let bytes = first.unwrap();
        let mut saved: Saved = serde_json::from_slice(&bytes).unwrap();
        let distinct = saved
            .runs
            .iter()
            .map(|run| &run.tasks)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        assert!(distinct >= 200, "schedule diversity regressed: {distinct}");
        evidence.push(json!({"case":name,"test":test,"binary":binary,"binary_sha256":v::sha256(fs::read(binary).unwrap()),
            "runs_sha256":v::sha256(&bytes),"unique_schedules":distinct}));
        // Replay a single standalone saved execution, without sampling new ones.
        saved.runs.truncate(1);
        let replay = directory.join(format!("{name}-single.json"));
        fs::write(&replay, serde_json::to_vec_pretty(&saved).unwrap()).unwrap();
        passed(
            child(binary, test, &directory.join("replay"))
                .env("CAPNTPROTO_SCHEDULE_REPLAY", &replay),
            &directory.join(format!("{name}-replay.log")),
        );
        // Both incomplete decisions and corrupted semantic expectations must fail.
        for (fault, message) in [
            ("truncated", "schedule ended early"),
            ("event", "schedule/event replay diverged"),
        ] {
            let mut bad = saved.clone();
            if fault == "truncated" {
                bad.runs[0].tasks.pop();
            } else {
                bad.runs[0].events[0].1 ^= 99;
            }
            let file = directory.join(format!("{name}-{fault}.json"));
            fs::write(&file, serde_json::to_vec_pretty(&bad).unwrap()).unwrap();
            let output = run(
                child(binary, test, &directory.join("controls"))
                    .env("CAPNTPROTO_SCHEDULE_REPLAY", file),
                &directory.join(format!("{name}-{fault}.log")),
                101,
            )
            .unwrap();
            assert!(
                output.contains(message),
                "wrong negative-control failure: {output}"
            );
        }
    }
    for (test, name, markers) in [
        (
            "native_shutdown::schedule_tests::shuttle_detects_lost_wakeup",
            "lost-wakeup-control",
            ["registered-with-lost-waker", "finished-without-wakeup"],
        ),
        (
            "native_rpc::session::shutdown_tests::shuttle_detects_lost_shutdown_terminal_wakeup",
            "shutdown-terminal-lost-wakeup",
            ["blocked-with-receipt", "terminal-canceled"],
        ),
    ] {
        let lost = directory.join(name);
        let output = run(
            child(&binaries["capntproto"], test, &lost).arg("--ignored"),
            &directory.join(format!("{name}.log")),
            101,
        )
        .unwrap();
        assert!(output.contains("deadlock! blocked tasks:"));
        let saved = lost.join(name).join("runs.json");
        let record: Saved = serde_json::from_slice(&fs::read(&saved).unwrap()).unwrap();
        assert_eq!(record.runs.len(), 1);
        for marker in markers {
            assert!(record.runs[0].events.contains(&(marker.into(), 1)));
        }
        assert!(!record.runs[0]
            .events
            .iter()
            .any(|(name, _)| name == "case-complete"));
        let output = run(
            child(
                &binaries["capntproto"],
                test,
                &directory.join(format!("{name}-replay")),
            )
            .arg("--ignored")
            .env("CAPNTPROTO_SCHEDULE_REPLAY", saved),
            &directory.join(format!("{name}-replay.log")),
            101,
        )
        .unwrap();
        assert!(output.contains("deadlock! blocked tasks:"));
    }
    assert_eq!(
        v::distribution::verification_hashes(&root()).unwrap(),
        inputs
    );
    fs::write(base.join("checked.json"), serde_json::to_vec_pretty(&json!({
        "format":1,"tool":"shuttle 0.9.4", "compiler":compiler,"inputs":inputs,"seed":SEED,
        "iterations_per_case":ITERATIONS,"processes":2,"same_process_replays":true,"features":["native"],
        "cases":evidence,"artifacts":directory,"controls":["lost-wakeup","replayed-lost-wakeup","terminal-lost-wakeup","replayed-terminal-lost-wakeup","truncated-decisions","changed-events"],
        "limits":"bounded single-thread polling of real watch/Notify futures, route workers, serializer/two-party output fences and shutdown completion with scheduled receipt/deadline signals; no instrumentation inside Tokio atomics, no weak-memory, actual clock/socket, authenticated route-installation, receipt-frame authentication or whole-runtime claim"
    })).unwrap()).unwrap();
}
