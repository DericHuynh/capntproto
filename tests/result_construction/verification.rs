use super::super::*;
use reproto_test_support::verification::{command, cpp, exploration, root as repo_root, run};

fn compare_cpp(inputs: &str, expected: &str) {
    let build = cpp::build(&["capnp-rpc", "capnp_tool", "capnpc_cpp"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = repo_root().join("target/verification/result-construction-cpp");
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
    let exe = tmp.path().join("result-construction");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", tmp.path().display()))
            .arg("tests/cpp/result-construction.c++")
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
    let input = tmp.path().join("cases.txt");
    std::fs::write(&input, inputs).unwrap();
    let output = run(command(exe).arg(input), &logs.join("reference.log"), 0).unwrap();
    let mut expected = expected.lines();
    let mut actual = output.lines();
    for case in inputs.lines() {
        for (step, _) in case.split_whitespace().skip(1).enumerate() {
            assert_eq!(actual.next(), expected.next(), "case {case} step {step}");
        }
    }
    assert!(actual.next().is_none() && expected.next().is_none());
    eprintln!(
        "{} C++ result ownership observations across {} cases",
        output.lines().count(),
        inputs.lines().count()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn tlc_result_root_ownership_traces() {
    const MODEL: &str = "verification/RpcResultOwnership.tla";
    const CONFIG: &str = include_str!("../../verification/RpcResultOwnership.cfg");
    let mut cpp_inputs = String::new();
    let mut cpp_expected = String::new();
    for path in exploration::traces(MODEL, "result-ownership", CONFIG).unwrap() {
        // Rust keeps even null owners tied to their arena. C++ permits a null
        // cross-message adoption, and releaseAs() may materialize a null value
        // before rejecting a wrong schema. Those error contracts are native-only.
        let comparable = !path
            .iter()
            .any(|s| matches!(s["event"], 7 | 8) && s["orphan"] == 3);
        for words in [0, 1, 64] {
            super::dynamic::replay_loaded(&path, words).await;
            if comparable {
                cpp_inputs += &words.to_string();
            }
            let live = Rc::new(Cell::new(0));
            let mut message =
                message::Builder::new(message::HeapAllocator::new().first_segment_words(words));
            let mut caps = Vec::new();
            let mut p: capnp::any_pointer::Builder = message.get_root().unwrap();
            p.imbue_mut(&mut caps);
            let (mut root, token) = Root::new(p, result_type()).unwrap().with_orphanage();
            let mut foreign = message::Builder::new_default();
            let (mut wrong, _) = Root::new(foreign.get_root().unwrap(), result_type())
                .unwrap()
                .with_orphanage();
            let mut orphan = None;
            for state in &path {
                if comparable {
                    cpp_inputs += &format!(" {}", state["event"]);
                    cpp_expected +=
                        &format!("{} {} {}\n", state["root"], state["orphan"], state["live"]);
                }
                match state["event"] {
                    k @ (1 | 2) => orphan = Some(new_orphan(&mut root, &token, k as u32, &live)),
                    3 => root.adopt(orphan.take().unwrap()).unwrap(),
                    4 => orphan = Some(root.disown(&token).unwrap()),
                    5 => drop(orphan.take()),
                    6 => root.clear(),
                    7 => {
                        let error = wrong.adopt(orphan.take().unwrap()).unwrap_err();
                        assert_eq!(error.error.kind, capnp::ErrorKind::WrongArena);
                        orphan = Some(error.orphan);
                        assert!(wrong.is_null());
                    }
                    8 => {
                        // Wrong schema conversion is checked before ownership
                        // transfer, just like a typed result-root adoption.
                        let error = orphan
                            .take()
                            .unwrap()
                            .release_as::<harness::value::Owned>()
                            .err()
                            .unwrap();
                        assert_eq!(error.error.kind, capnp::ErrorKind::TypeMismatch);
                        orphan = Some(error.orphan);
                    }
                    9 => {
                        let held = root.disown(&token).unwrap();
                        let mut access = token.in_root(&mut root).unwrap();
                        let mut held = held
                            .release_as::<harness::pending_results::Owned>()
                            .unwrap();
                        // A scoped view cannot borrow the arena twice; copy via
                        // an independent message while retaining the same cap.
                        let cap = access.read_typed(&mut held, |r| r.get_cap()).unwrap();
                        let mut source =
                            capnp_rpc::PipelineBuilder::<harness::pending_results::Owned>::new();
                        source.get().set_cap(cap);
                        let cloned = access
                            .copy(dynamic_value::Reader::from(source.get().into_reader()))
                            .unwrap();
                        root.adopt(held.into_dynamic()).unwrap();
                        orphan = Some(cloned);
                    }
                    10 => {
                        orphan = Some(
                            token
                                .in_root(&mut root)
                                .unwrap()
                                .null(result_type())
                                .unwrap(),
                        )
                    }
                    _ => panic!(),
                }
                assert_eq!(
                    u64::from(value(root_cap(&root)).await),
                    state["root"],
                    "{state:?}"
                );
                assert_eq!(u64::from(live.get()), state["live"], "{state:?}");
                if let Some(held) = &mut orphan {
                    let mut access = token.in_root(&mut root).unwrap();
                    if state["orphan"] == 3 {
                        assert!(access.is_null(held).unwrap());
                    } else {
                        let cap = access
                            .read(held, |v| {
                                v.downcast::<capnp::dynamic_struct::Reader>()
                                    .downcast::<harness::pending_results::Owned>()
                                    .get_cap()
                            })
                            .unwrap();
                        assert_eq!(u64::from(value(Some(cap)).await), state["orphan"]);
                    }
                } else {
                    assert_eq!(state["orphan"], 0);
                }
            }
            drop(orphan);
            root.clear();
            assert_eq!(live.get(), 0);
            if comparable {
                cpp_inputs.push('\n');
            }
        }
    }
    compare_cpp(&cpp_inputs, &cpp_expected);
    exploration::controls(
        MODEL,
        "result-ownership",
        CONFIG,
        &[
            ("loseCapability", "CapabilityOwnership"),
            ("duplicateOwner", "AdoptionMoves"),
            ("allowForeign", "FailurePreserves"),
            ("consumeOnError", "FailurePreserves"),
        ],
        None,
    )
    .unwrap();
}
