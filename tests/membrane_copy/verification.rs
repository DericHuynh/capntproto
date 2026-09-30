use super::super::*;
use reproto_test_support::verification::{command, cpp, exploration, root, run};

fn cpp_replay(inputs: &str, expected: &str) {
    let build = cpp::build(&["capnpc", "capnp_tool", "capnpc_cpp", "capnp-rpc"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/membrane-copy-cpp");
    let temp = tempfile::tempdir().unwrap();
    run(
        command(bin.join("capnp")).args([
            "compile",
            &format!(
                "-o{}:{}",
                bin.join("capnpc-c++").display(),
                temp.path().display()
            ),
            "-Ivendor/capnproto/c++/src",
            "--src-prefix=schemas",
            "schemas/membrane-copy.capnp",
        ]),
        &logs.join("schema.log"),
        0,
    )
    .unwrap();
    let exe = temp.path().join("membrane-copy");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", temp.path().display()))
            .arg("tests/cpp/membrane-copy.c++")
            .arg(temp.path().join("membrane-copy.capnp.c++"))
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
    let input = logs.join("cases.txt");
    std::fs::write(&input, inputs).unwrap();
    let output = run(command(exe).arg(input), &logs.join("reference.log"), 0).unwrap();
    let mut actual = output.lines();
    let mut expected = expected.lines();
    for case in inputs.lines() {
        assert_eq!(actual.next(), expected.next(), "{case}");
    }
    assert!(actual.next().is_none() && expected.next().is_none());
}

#[test]
fn tlc_membrane_copy_lifetimes_and_directions_match_cpp() {
    const MODEL: &str = "verification/MembraneCopy.tla";
    const CONFIG: &str = include_str!("../../verification/MembraneCopy.cfg");
    let paths = exploration::traces(MODEL, "membrane-copy", CONFIG).unwrap();
    let traces = paths.len();
    let mut inputs = String::new();
    let mut expected = String::new();
    let mut observations = 0;
    for form in 0..4 {
        for path in &paths {
            let stats = Rc::new(Stats::default());
            let cap = server(&stats);
            let original = cap.as_client_hook().get_ptr();
            let mut source = Message::source(form, cap);
            let membrane = Membrane::new(Rc::new(Boundary(stats.clone())));
            let mut destination = Message::default();
            let (mut target, token) = destination.root();
            // Separate arena keeps reverse-copy reads and writes disjoint.
            let mut returned = Message::default();
            let (mut returned_root, returned_token) = returned.root();
            let mut orphan = None;
            let mut back = None;
            inputs += &format!("{form} ");
            for state in path {
                let calls = stats.calls.get();
                let event = state["event"];
                inputs += &format!("{event} ");
                match event {
                    1 | 2 => {
                        orphan = Some(
                            copied(
                                form,
                                &membrane,
                                event == 2,
                                source.reader(),
                                &mut token.in_root(&mut target).unwrap(),
                            )
                            .unwrap(),
                        );
                    }
                    3 => {
                        if let Some(owner) = orphan.take() {
                            target.adopt(owner).unwrap();
                        } else {
                            returned_root.adopt(back.take().unwrap()).unwrap();
                            let copied = token
                                .in_root(&mut target)
                                .unwrap()
                                .copy(root_reader(&returned_root).into())
                                .unwrap();
                            target.adopt(copied).unwrap();
                            returned_root.clear();
                        }
                    }
                    4 | 5 => {
                        back = Some(
                            copied(
                                form,
                                &membrane,
                                event == 4,
                                root_reader(&target),
                                &mut returned_token.in_root(&mut returned_root).unwrap(),
                            )
                            .unwrap(),
                        );
                    }
                    6 => {
                        orphan = None;
                        back = None;
                    }
                    7 => target.clear(),
                    8 => source.builder().clear(),
                    9 => membrane.revoke(Error::failed("revoked".into())),
                    _ => panic!(),
                }
                assert_eq!(
                    stats.calls.get(),
                    calls,
                    "copying/adoption must not call the service"
                );
                let s = if source.reader().is_null() {
                    0
                } else {
                    observe(read_caps(form, source.reader()).unwrap(), &stats, original)
                };
                let d = if target.is_null() {
                    0
                } else {
                    observe(
                        read_caps(form, root_reader(&target)).unwrap(),
                        &stats,
                        original,
                    )
                };
                let o = if let Some(owner) = orphan.as_mut() {
                    observe(
                        orphan_caps(form, &mut target, &token, owner),
                        &stats,
                        original,
                    )
                } else if let Some(owner) = back.as_mut() {
                    observe(
                        orphan_caps(form, &mut returned_root, &returned_token, owner),
                        &stats,
                        original,
                    )
                } else {
                    0
                };
                let live = stats.live.get();
                assert_eq!(
                    [s, d, o, live],
                    [
                        state["seenSource"],
                        state["seenDest"],
                        state["seenOrphan"],
                        state["live"]
                    ],
                    "form {form}, {path:?}"
                );
                expected += &format!("{s} {d} {o} {live},");
                observations += 1;
            }
            inputs.push('\n');
            expected.push('\n');
            drop(orphan);
            drop(back);
            target.clear();
            returned_root.clear();
            source.builder().clear();
            assert_eq!(stats.live.get(), 0);
        }
    }
    eprintln!(
        "{observations} Rust/C++ membrane-copy observations across {} scenarios",
        traces * 4
    );
    cpp_replay(&inputs, &expected);
    exploration::controls(
        MODEL,
        "membrane-copy",
        CONFIG,
        &[
            ("swapDirection", "Crossing"),
            ("dropCaps", "Crossing"),
            ("skipUnwrap", "Crossing"),
            ("dropOnAdopt", "Crossing"),
            ("stealSource", "SourceUnchanged"),
            ("ignoreRevocation", "Crossing"),
            ("retainDropped", "AuthorityLifetime"),
            ("resurrect", "Crossing"),
        ],
        None,
    )
    .unwrap();
}
