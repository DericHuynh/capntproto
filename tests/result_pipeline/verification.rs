use super::super::*;
use capntproto_test_support::verification::{command, cpp, exploration, root, run};

fn compare_cpp(inputs: &str, actual: &str) {
    let build = cpp::build(&["capnp-rpc", "capnp_tool", "capnpc_cpp"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/independent-pipeline-cpp");
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
        ]),
        &logs.join("schema.log"),
        0,
    )
    .unwrap();
    let exe = tmp.path().join("result-pipeline");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", tmp.path().display()))
            .arg("tests/cpp/result-pipeline.c++")
            .arg(tmp.path().join("runtime-test.capnp.c++"))
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
    let input = tmp.path().join("events.txt");
    std::fs::write(&input, inputs).unwrap();
    let reference = run(command(exe).arg(input), &logs.join("reference.log"), 0).unwrap();
    let mut expected = reference.lines();
    let mut got = actual.lines();
    for scenario in inputs.lines() {
        for (step, event) in scenario.split_whitespace().skip(1).enumerate() {
            assert_eq!(
                got.next(),
                expected.next(),
                "scenario {scenario}, step {step}, event {event}"
            );
        }
    }
    assert!(expected.next().is_none() && got.next().is_none());
    eprintln!(
        "{} C++ pipeline observations over {} scenarios",
        actual.lines().count(),
        inputs.lines().count()
    );
}

fn apply(f: &mut Fixture, event: u64) {
    match event {
        1 => f.publish().unwrap(),
        2 => f.resolve.take().unwrap().send(true).unwrap(),
        3 => f.resolve.take().unwrap().send(false).unwrap(),
        4 => f.finish(true),
        5 => f.finish(false),
        6 => f.call(),
        7 => f.duplicate(),
        8 => f.observe(),
        _ => panic!(),
    }
}
fn snapshot(f: &Fixture) -> String {
    format!(
        "{} {} {} {} {}\n",
        f.done,
        f.failed,
        f.parent_result,
        f.calls.get(),
        f.observed
    )
}

#[tokio::test(flavor = "current_thread")]
async fn tlc_independent_pipeline_traces() {
    const MODEL: &str = "verification/RpcIndependentPipeline.tla";
    const CONFIG: &str = include_str!("../../verification/RpcIndependentPipeline.cfg");
    let mut inputs = String::new();
    let mut actual = String::new();
    for path in exploration::traces(MODEL, "independent-pipeline", CONFIG).unwrap() {
        inputs.push('0');
        let mut f = Fixture::new(false).await;
        for state in path {
            inputs += &format!(" {}", state["event"]);
            apply(&mut f, state["event"]);
            f.drive().await;
            assert_eq!(
                (f.done, f.failed, f.parent_result),
                (state["done"], state["failed"], state["returned"]),
                "{state:?}"
            );
            assert_eq!(u64::from(f.calls.get()), state["done"]);
            assert_eq!(f.observed, state["observed"], "{state:?}");
            actual += &snapshot(&f);
        }
        inputs.push('\n');
    }
    // RPC Return(exception) can break fresh caller-side pipeline projections,
    // while an already forwarded child is an independent question. Exercise
    // both return/target orderings, including rejected target promises.
    tokio::task::LocalSet::new()
        .run_until(async {
            for returned in [4, 5] {
                for offered in [2, 3] {
                    for return_first in [false, true] {
                        let mut f = Fixture::new(true).await;
                        let order = if return_first {
                            [returned, offered]
                        } else {
                            [offered, returned]
                        };
                        inputs.push('1');
                        for event in [6, 8, 1, 7, order[0], 6, order[1], 6] {
                            inputs += &format!(" {event}");
                            apply(&mut f, event);
                            f.drive().await;
                            actual += &snapshot(&f);
                        }
                        inputs.push('\n');
                    }
                }
            }
        })
        .await;
    compare_cpp(&inputs, &actual);
    exploration::controls(
        MODEL,
        "independent-pipeline",
        CONFIG,
        &[
            ("completeOnPublish", "PublicationIsIndependent"),
            ("waitForReturn", "CorrectRouting"),
            ("revokeOnError", "CorrectRouting"),
            ("replacePublished", "PublicationIsSingleUse"),
            ("delayObserver", "CorrectResolution"),
        ],
        None,
    )
    .unwrap();
}
