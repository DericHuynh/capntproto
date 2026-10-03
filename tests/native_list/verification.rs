use super::super::*;
use capntproto_test_support::verification::{command, cpp, exploration, root, run};
fn compare_cpp(inputs: &str, expected: &str) {
    let build = cpp::build(&["capnpc", "capnp-rpc", "capnp_tool", "capnpc_cpp"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/native-list-cpp");
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
            "schemas/native-list.capnp",
        ]),
        &logs.join("schema.log"),
        0,
    )
    .unwrap();
    let exe = tmp.path().join("native-list");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", tmp.path().display()))
            .arg("tests/cpp/native-list.c++")
            .arg(tmp.path().join("native-list.capnp.c++"))
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
fn tlc_registration_and_native_storage_traces_match_cpp() {
    const MODEL: &str = "verification/NativeListCast.tla";
    const CONFIG: &str = include_str!("../../verification/NativeListCast.cfg");
    let paths = exploration::traces(MODEL, "native-list-cast", CONFIG).unwrap();
    let plain = unregistered();
    let mut inputs = String::new();
    let mut expected = String::new();
    let mut observations = 0;
    let traces = paths.len();
    for path in paths {
        let mut loader = plain.clone();
        let mut selected = 0;
        let mut message = make_message(&loader, selected);
        for state in &path {
            let event = state["event"];
            inputs += &format!("{event} ");
            let result = match event {
                1..=8 => {
                    selected = event - 1;
                    message = make_message(&loader, selected);
                    false
                }
                9..=11 => {
                    register(&mut loader, event - 9);
                    false
                }
                12 => cast_reader(&message, &loader, selected),
                13 => cast_builder(&mut message, &loader, selected, true),
                _ => panic!("{state:?}"),
            };
            let actual = value(&message, selected);
            assert_eq!(selected, state["selected"]);
            assert_eq!(
                (u64::from(result), actual),
                (state["result"], state["value"]),
                "{path:?}"
            );
            expected += &format!("{} {actual},", u8::from(result));
            observations += 1;
        }
        inputs.push('\n');
        expected.push('\n');
    }
    compare_cpp(&inputs, &expected);
    eprintln!("{observations} Rust/C++ native-list observations across {traces} scenarios");
    exploration::controls(
        MODEL,
        "native-list-cast",
        CONFIG,
        &[
            ("skipRegistration", "CastResult"),
            ("checkBrand", "CastResult"),
            ("wrongKind", "CastResult"),
            ("wrongId", "CastResult"),
            ("wrongDepth", "CastResult"),
            ("mutateRejected", "SameStorage"),
            ("copyWrite", "SameStorage"),
        ],
        None,
    )
    .unwrap();
}
