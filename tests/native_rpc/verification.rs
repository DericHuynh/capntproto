use super::super::*;
use reproto_test_support::verification::{command, cpp, exploration, root, run};

fn compare_cpp(inputs: &str, expected: &str) {
    let build = cpp::build(&["capnpc", "capnp-rpc", "capnp_tool", "capnpc_cpp"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/native-rpc-cpp");
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
            "schemas/native-rpc.capnp",
        ]),
        &logs.join("schema.log"),
        0,
    )
    .unwrap();
    let exe = tmp.path().join("native-rpc");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", tmp.path().display()))
            .arg("tests/cpp/native-rpc.c++")
            .arg(tmp.path().join("native-rpc.capnp.c++"))
            .arg(bin.join("libcapnpc.a"))
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
fn tlc_native_ownership_and_calls_match_cpp() {
    const MODEL: &str = "verification/NativeRpcCast.tla";
    const CONFIG: &str = include_str!("../../verification/NativeRpcCast.cfg");
    let paths = exploration::traces(MODEL, "native-rpc-cast", CONFIG).unwrap();
    let mut inputs = String::new();
    let mut expected = String::new();
    let mut observations = 0;
    let traces = paths.len();
    for path in paths {
        let initial = &path[0];
        let selected = initial["selected"];
        let loader = loader(initial["registered"] == 1);
        let mode = initial["mode"] == 1;
        let alive = Rc::new(Cell::new(false));
        let calls = Rc::new(Cell::new(0));
        let mut source = None;
        let mut native = None;
        let mut result = 0;
        let mut returned = 0;
        for state in &path {
            let event = state["event"];
            inputs += &format!("{event} ");
            match event {
                1..=28 => {
                    source = Some(super::super::source(
                        &loader,
                        selected,
                        mode,
                        alive.clone(),
                        calls.clone(),
                    ))
                }
                31 => match share(source.as_ref().unwrap(), selected) {
                    Ok(n) => {
                        native = Some(n);
                        result = 1;
                    }
                    Err(_) => result = 2,
                },
                32 => match release(source.take().unwrap(), selected) {
                    Ok(n) => {
                        native = Some(n);
                        result = 1;
                    }
                    Err((_, s)) => {
                        source = Some(*s);
                        result = 2;
                    }
                },
                33 => {
                    source.take();
                }
                34 => {
                    native.take();
                }
                35 => returned = futures::executor::block_on(call(native.as_ref().unwrap())) as u64,
                _ => panic!(),
            }
            let actual = [
                u64::from(source.is_some()),
                u64::from(native.is_some()),
                u64::from(alive.get()),
                result,
                calls.get(),
                returned,
            ];
            assert_eq!(
                actual,
                [
                    state["source"],
                    state["native"],
                    state["alive"],
                    state["result"],
                    state["calls"],
                    state["returned"]
                ],
                "{path:?}"
            );
            expected += &format!(
                "{} {} {} {} {} {},",
                actual[0], actual[1], actual[2], actual[3], actual[4], actual[5]
            );
            observations += 1;
        }
        drop(source);
        drop(native);
        assert!(!alive.get());
        inputs.push('\n');
        expected.push('\n');
    }
    compare_cpp(&inputs, &expected);
    eprintln!("{observations} Rust/C++ native RPC observations across {traces} scenarios");
    exploration::controls(
        MODEL,
        "native-rpc-cast",
        CONFIG,
        &[
            ("skipRegistration", "Compatibility"),
            ("checkBrand", "Compatibility"),
            ("wrongInterface", "Compatibility"),
            ("wrongStruct", "Compatibility"),
            ("consumeRejected", "RejectedOwner"),
            ("cloneOnRelease", "Transfer"),
            ("lostHook", "Retention"),
            ("leakNative", "Retention"),
            ("lostPath", "CallResult"),
        ],
        None,
    )
    .unwrap();
}
