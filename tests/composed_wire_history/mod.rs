//! State-relative choices stay legal when Proptest removes earlier decisions.
//! Every concrete history must be a path in the fresh composed-handler graph.
use super::replay;
use proptest::{
    prelude::*,
    test_runner::{Config, RngAlgorithm, RngSeed, TestError, TestRunner},
};
use reproto_test_support::verification::{
    self as v,
    exploration::{self, Graph, State},
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, fs, path::Path, time::Duration};

const MODEL: &str = "verification/ComposedWireBoundary.tla";
const CONFIG: &str = include_str!("../../verification/ComposedWireHistory.cfg");
const SEED: u64 = 0x72706368697374;
const TEST: &str = "composed_wire_history::generated_rpc_histories";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    pick: u16,
    yields: u8,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Ending {
    ReleaseAB,
    ReleaseBA,
    OverRelease,
    UnknownExport,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    start: u8,
    decisions: Vec<Decision>,
    ending: Ending,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    format: u32,
    graph: v::GraphDigest,
    case: Case,
    states: Vec<State>,
    yields: Vec<u8>,
}

fn graph() -> Graph {
    exploration::explore(MODEL, "composed-wire-history", CONFIG).unwrap()
}
fn strategy() -> impl Strategy<Value = Case> {
    (
        any::<u8>(),
        proptest::collection::vec(
            (any::<u16>(), 0u8..4).prop_map(|(pick, yields)| Decision { pick, yields }),
            0..65,
        ),
        prop_oneof![
            Just(Ending::ReleaseAB),
            Just(Ending::ReleaseBA),
            Just(Ending::OverRelease),
            Just(Ending::UnknownExport)
        ],
    )
        .prop_map(|(start, decisions, ending)| Case {
            start,
            decisions,
            ending,
        })
}
fn runner() -> TestRunner {
    TestRunner::new(Config {
        cases: 128,
        rng_algorithm: RngAlgorithm::ChaCha,
        rng_seed: RngSeed::Fixed(SEED),
        max_shrink_iters: 4096,
        max_shrink_time: 30_000,
        failure_persistence: None,
        ..Config::default()
    })
}
fn release(graph: &Graph, states: &mut Vec<State>, id: u64, amount: u64) {
    let next = graph
        .successors(states.last().unwrap())
        .unwrap()
        .iter()
        .find(|s| s["event"] == 2 && s["id"] == id && s["amount"] == amount)
        .unwrap();
    states.push(next.clone());
}
fn artifact(graph: &Graph, case: Case) -> Artifact {
    assert!(case.decisions.len() <= 64 && case.decisions.iter().all(|d| d.yields < 4));
    let starts = graph.successors(graph.initial()).unwrap();
    let mut states = vec![starts[case.start as usize % starts.len()].clone()];
    let mut yields = vec![0];
    for decision in &case.decisions {
        // Keep both exports live during the variable-length prefix, so a
        // malformed early Release cannot hide the rest of the generated work.
        let next: Vec<_> = graph
            .successors(states.last().unwrap())
            .unwrap()
            .iter()
            .filter(|s| s["live"] == 1 && s["a"] > 0 && s["b"] > 0)
            .collect();
        states.push(next[decision.pick as usize % next.len()].clone());
        yields.push(decision.yields);
    }
    match case.ending {
        Ending::ReleaseAB | Ending::ReleaseBA => {
            let order = if case.ending == Ending::ReleaseAB {
                [1, 2]
            } else {
                [2, 1]
            };
            for id in order {
                let count = states.last().unwrap()[if id == 1 { "a" } else { "b" }];
                release(graph, &mut states, id, count);
            }
            // A zero-count Release of a retired slot must still abort.
            release(graph, &mut states, order[0], 0);
        }
        Ending::OverRelease => {
            let count = states.last().unwrap()["a"] + 1;
            release(graph, &mut states, 1, count);
        }
        Ending::UnknownExport => release(graph, &mut states, 0, 0),
    }
    yields.resize(states.len(), 0);
    graph.validate(&states).unwrap();
    assert_eq!(states.last().unwrap()["live"], 0);
    Artifact {
        format: 1,
        graph: graph.digest().clone(),
        case,
        states,
        yields,
    }
}
fn validate(
    graph: &Graph,
    saved: &Artifact,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if saved.format != 1 || &saved.graph != graph.digest() {
        return Err("unsupported artifact or changed model graph".into());
    }
    if saved.case.decisions.len() > 64 || saved.case.decisions.iter().any(|d| d.yields >= 4) {
        return Err("history decisions exceed the replay bounds".into());
    }
    graph.validate(&saved.states)?;
    if *saved != artifact(graph, saved.case.clone()) {
        return Err("changed or truncated replay".into());
    }
    Ok(())
}
fn execute(runtime: &tokio::runtime::Runtime, saved: &Artifact) {
    // Each trial/shrink attempt gets fresh runtime state and a fresh local task
    // set; cancellation of a failed trial cannot leak tasks into the next one.
    let local = tokio::task::LocalSet::new();
    runtime.block_on(local.run_until(async {
        tokio::time::timeout(Duration::from_secs(5), replay(&saved.states, &saved.yields))
            .await
            .unwrap();
    }));
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn save_failure(saved: &Artifact) -> std::path::PathBuf {
    let bytes = serde_json::to_vec_pretty(saved).unwrap();
    let directory = v::root().join("target/verification/composed-wire-history/failures");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{}.json", v::sha256(&bytes)));
    fs::write(&path, bytes).unwrap();
    path
}

fn worker(report: &Path) {
    let sources = v::distribution::verification_hashes(&v::root()).unwrap();
    let graph = graph();
    let runtime = runtime();
    let evidence = RefCell::new(Vec::new());
    let result = runner().run(&strategy(), |case| {
        let saved = artifact(&graph, case);
        execute(&runtime, &saved);
        let copy: Artifact = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        validate(&graph, &copy).unwrap();
        execute(&runtime, &copy);
        evidence.borrow_mut().push(saved);
        Ok(())
    });
    if let Err(error) = result {
        if let TestError::Fail(_, case) = &error {
            let saved = artifact(&graph, case.clone());
            let path = save_failure(&saved);
            eprintln!("replay: REPROTO_RPC_HISTORY_REPLAY={} cargo test --test composed_wire_boundary {TEST} -- --exact --nocapture",path.display());
        }
        panic!("RPC history property failed: {error}");
    }
    let evidence = evidence.into_inner();
    assert_eq!(evidence.len(), 128);
    // Reject shallow/vacuous generator changes even when all cases pass.
    assert!(evidence.iter().any(|a| a.case.decisions.len() >= 48));
    for a in 1..=3 {
        for b in 1..=3 {
            assert!(evidence
                .iter()
                .any(|h| h.states[0]["a"] == a && h.states[0]["b"] == b));
        }
    }
    for yields in 0..4 {
        assert!(evidence.iter().any(|a| a.yields.contains(&yields)));
    }
    for ending in [
        Ending::ReleaseAB,
        Ending::ReleaseBA,
        Ending::OverRelease,
        Ending::UnknownExport,
    ] {
        assert!(evidence.iter().any(|a| a.case.ending == ending));
    }
    for id in 1..=2 {
        for method in 0..=1 {
            for data in 0..=1 {
                assert!(evidence
                    .iter()
                    .flat_map(|a| &a.states)
                    .any(|s| s["event"] == 3
                        && s["id"] == id
                        && s["method"] == method
                        && s["data"] == data));
            }
        }
    }
    assert!(
        evidence
            .iter()
            .any(|a| a.states.windows(2).any(|s| s[0] == s[1])),
        "repeat a modeled cycle"
    );
    assert!(
        evidence.iter().any(|a| a
            .states
            .iter()
            .skip(1)
            .any(|s| s["event"] == 2 && s["amount"] > 0 && s["a"] > 0 && s["b"] > 0)),
        "partial release"
    );
    assert_eq!(
        sources,
        v::distribution::verification_hashes(&v::root()).unwrap(),
        "sources changed during qualification"
    );
    let compiler = std::process::Command::new("rustc")
        .arg("-Vv")
        .output()
        .unwrap();
    assert!(compiler.status.success());
    let report_value = serde_json::json!({ "format":1, "seed":SEED, "config":CONFIG,
        "graph":graph.digest(), "sources":sources,
        "compiler":String::from_utf8(compiler.stdout).unwrap(),
        "binary_sha256":v::sha256(fs::read(std::env::current_exe().unwrap()).unwrap()),
        "scope":"sequential RPC handler histories; task yields, not executor enumeration; no crypto, handoff or reconnect",
        "histories":evidence });
    fs::write(report, serde_json::to_vec(&report_value).unwrap()).unwrap();
}

#[test]
fn generated_rpc_histories() {
    if let Some(path) = std::env::var_os("REPROTO_RPC_HISTORY_REPLAY") {
        let graph = graph();
        let saved: Artifact = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        validate(&graph, &saved).unwrap();
        execute(&runtime(), &saved);
        return;
    }
    if let Some(path) = std::env::var_os("REPROTO_RPC_HISTORY_WORKER_REPORT") {
        worker(Path::new(&path));
        return;
    }
    let directory = v::root().join("target/verification/composed-wire-history");
    fs::create_dir_all(&directory).unwrap();
    let mut reports = vec![];
    for index in 0..2 {
        let report = directory.join(format!("histories-{index}.json"));
        if report.exists() {
            fs::remove_file(&report).unwrap();
        }
        let output = v::run(
            v::command(std::env::current_exe().unwrap())
                .args([TEST, "--exact", "--nocapture"])
                .env("REPROTO_RPC_HISTORY_WORKER_REPORT", &report),
            &directory.join(format!("worker-{index}.log")),
            0,
        )
        .unwrap();
        assert!(
            output.contains("test result: ok. 1 passed; 0 failed;"),
            "worker selected no test: {output}"
        );
        reports.push(fs::read(report).unwrap());
    }
    assert_eq!(
        reports[0], reports[1],
        "independent processes generated different histories or provenance"
    );
    let report: serde_json::Value = serde_json::from_slice(&reports[0]).unwrap();
    eprintln!("RPC histories: 128 cases, each replayed twice in two processes; graph {} states / {} edges",report["graph"]["states"],report["graph"]["edges"]);
}

#[test]
fn shrinking_and_saved_history_controls() {
    let graph = graph();
    let result = runner().run(&strategy(), |case| {
        let saved = artifact(&graph, case);
        let saw = |method| {
            saved
                .states
                .iter()
                .any(|s| s["event"] == 3 && s["method"] == method)
        };
        if saw(0) && saw(1) {
            Err(TestCaseError::fail("synthetic method pair"))
        } else {
            Ok(())
        }
    });
    let TestError::Fail(reason, case) = result.unwrap_err() else {
        panic!("expected shrunk failure")
    };
    assert_eq!(reason.to_string(), "synthetic method pair");
    assert_eq!(case.decisions.len(), 2);
    assert!(case.decisions.iter().all(|d| d.yields == 0));
    let saved = artifact(&graph, case);
    let path = save_failure(&saved);
    let copy: Artifact = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    validate(&graph, &copy).unwrap();
    execute(&runtime(), &copy); // the synthetic failure is not a protocol defect
    let output = v::run(
        v::command(std::env::current_exe().unwrap())
            .args([TEST, "--exact", "--nocapture"])
            .env("REPROTO_RPC_HISTORY_REPLAY", path),
        &v::root().join("target/verification/composed-wire-history/saved-replay.log"),
        0,
    )
    .unwrap();
    assert!(
        output.contains("test result: ok. 1 passed; 0 failed;"),
        "saved replay selected no test: {output}"
    );
    let mut truncated = copy.clone();
    truncated.states.pop();
    truncated.yields.pop();
    assert!(validate(&graph, &truncated).is_err());
    let mut altered = copy.clone();
    altered.states[1].insert("seenData".into(), 99);
    assert!(validate(&graph, &altered).is_err());
    let mut altered = copy;
    altered.yields[0] = 1;
    assert!(validate(&graph, &altered).is_err());
}
