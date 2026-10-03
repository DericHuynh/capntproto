use super::super::*;
use capntproto_test_support::verification::{command, cpp, exploration, root, run};

fn compare_cpp(inputs: &str, expected: &str) {
    let build = cpp::build(&["capnp-rpc", "capnp_tool", "capnpc_cpp"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/ignore-result-cpp");
    let tmp = tempfile::tempdir().unwrap();
    run(
        command(bin.join("capnp")).args([
            "compile",
            &format!(
                "-o{}:{}",
                bin.join("capnpc-c++").display(),
                tmp.path().display()
            ),
            "-Ivendor/capnproto/c++/src",
            "--src-prefix=schemas",
            "schemas/runtime-test.capnp",
            "schemas/cancellation-policy.capnp",
        ]),
        &logs.join("schema.log"),
        0,
    )
    .unwrap();
    let exe = tmp.path().join("ignore-result");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", tmp.path().display()))
            .arg("tests/cpp/ignore-result.c++")
            .arg(tmp.path().join("runtime-test.capnp.c++"))
            .arg(tmp.path().join("cancellation-policy.capnp.c++"))
            .arg(bin.join("libcapnp-rpc.a"))
            .arg(bin.join("libcapnp.a"))
            .arg(build.join("c++/src/kj/libkj-async.a"))
            .arg(build.join("c++/src/kj/libkj.a"))
            .args(["-pthread", "-o"])
            .arg(&exe),
        &logs.join("compile.log"),
        0,
    )
    .unwrap();
    let input = tmp.path().join("cases.txt");
    std::fs::write(&input, inputs).unwrap();
    let output = run(command(exe).arg(input), &logs.join("reference.log"), 0).unwrap();
    let mut actual = output.lines();
    let mut expected = expected.lines();
    for case in inputs.lines() {
        for (step, _) in case.split_whitespace().skip(1).enumerate() {
            assert_eq!(actual.next(), expected.next(), "case {case}, step {step}");
        }
    }
    assert!(actual.next().is_none() && expected.next().is_none());
    eprintln!(
        "{} C++ ignore-result observations across {} cases",
        output.lines().count(),
        inputs.lines().count()
    );
}
#[tokio::test(flavor = "current_thread")]
async fn tlc_ignore_result_lifecycle_traces() {
    const MODEL: &str = "verification/RpcIgnoreResult.tla";
    const CONFIG: &str = include_str!("../../verification/RpcIgnoreResult.cfg");
    let paths = exploration::traces(MODEL, "ignore-result", CONFIG).unwrap();
    let mut inputs = String::new();
    let mut expected = String::new();
    tokio::task::LocalSet::new()
        .run_until(async {
            for path in &paths {
                for wire in [false, true] {
                    inputs += if wire { "1" } else { "0" };
                    let mut fixture = None;
                    for state in path {
                        inputs += &format!(" {}", state["event"]);
                        if state["event"] <= 2 {
                            assert!(fixture.is_none());
                            fixture =
                                Some(Fixture::new(wire, state["cancellable"] == 1, 0, 0).await);
                        } else {
                            fixture.as_mut().unwrap().step(state["event"]).await;
                        }
                        let observed = fixture.as_ref().unwrap().observation();
                        let wanted = [
                            state["held"],
                            state["running"],
                            state["completed"],
                            state["outcome"],
                            state["alive"],
                            state["results"],
                        ];
                        assert_eq!(
                            observed, wanted,
                            "wire={wire}, path={path:?}, state={state:?}"
                        );
                        expected += &wanted.map(|v| v.to_string()).join(" ");
                        expected.push('\n');
                    }
                    inputs.push('\n');
                    fixture.unwrap().close().await;
                }
            }
        })
        .await;
    compare_cpp(&inputs, &expected);
    exploration::controls(
        MODEL,
        "ignore-result",
        CONFIG,
        &[
            ("earlySuccess", "CompletionObserved"),
            ("earlyRelease", "CapabilityLifetime"),
            ("leakResult", "CapabilityLifetime"),
            ("swallowFailure", "CompletionObserved"),
            ("ignoreCancellation", "CancellationAllowed"),
            ("cancelProtected", "ProtectedCompletion"),
        ],
        None,
    )
    .unwrap();
}
