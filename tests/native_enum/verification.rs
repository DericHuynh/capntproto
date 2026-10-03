use super::super::*;
use capntproto_test_support::verification::{command, cpp, exploration, root, run};

fn compare_cpp(inputs: &str, expected: &str) {
    let build = cpp::build(&["capnpc", "capnp_tool", "capnpc_cpp"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/native-enum-cpp");
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
            "schemas/enum-brand.capnp",
        ]),
        &logs.join("schema.log"),
        0,
    )
    .unwrap();
    let exe = tmp.path().join("native-enum");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", tmp.path().display()))
            .arg("tests/cpp/native-enum.c++")
            .arg(tmp.path().join("enum-brand.capnp.c++"))
            .arg(bin.join("libcapnpc.a"))
            .arg(bin.join("libcapnp.a"))
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
fn tlc_individual_enum_casts_and_source_members_match_cpp() {
    const MODEL: &str = "verification/NativeEnumCast.tla";
    const CONFIG: &str = include_str!("../../verification/NativeEnumCast.cfg");
    let paths = exploration::traces(MODEL, "native-enum-cast", CONFIG).unwrap();
    let traces = paths.len();
    let catalog = Catalog::new();
    let mut inputs = String::new();
    let mut expected = String::new();
    let mut observations = 0;
    for path in paths {
        let mut message = message::Builder::new_default();
        message
            .init_root::<fixture::record::Builder>()
            .set_other(77);
        let mut selected = 0;
        let mut ordinal = 0;
        let mut result = 0;
        let mut inspected = 0;
        for state in &path {
            let event = state["event"];
            inputs += &format!("{event} ");
            match event {
                1..=7 => selected = event - 1,
                11..=14 => ordinal = [0, 1, 2, u16::MAX][(event - 11) as usize],
                21 => match catalog.cast(selected, ordinal) {
                    Ok(value) => {
                        write(&mut message, value);
                        result = 1;
                    }
                    Err(error) => {
                        mismatch::<OpenTone>(Err(error));
                        result = 2;
                    }
                },
                22 => {
                    inspected = match catalog.inspect(selected, ordinal) {
                        Ok(Some(index)) => {
                            assert_eq!(index, ordinal);
                            1
                        }
                        Ok(None) => 2,
                        Err(error) => {
                            assert_eq!(selected, 6);
                            mismatch::<()>(Err(error));
                            3
                        }
                    }
                }
                _ => panic!("unexpected event {event}"),
            }
            let (tag, stored) = stored(&message);
            let actual = [selected, u64::from(ordinal), tag, stored, result, inspected];
            assert_eq!(
                actual,
                [
                    state["selected"],
                    state["ordinal"],
                    state["tag"],
                    state["stored"],
                    state["result"],
                    state["inspected"]
                ],
                "{path:?}"
            );
            expected += &format!(
                "{} {} {} {} {} {},",
                actual[0], actual[1], actual[2], actual[3], actual[4], actual[5]
            );
            observations += 1;
        }
        inputs.push('\n');
        expected.push('\n');
    }
    compare_cpp(&inputs, &expected);
    eprintln!("{observations} Rust/C++ native enum observations across {traces} scenarios");
    exploration::controls(
        MODEL,
        "native-enum-cast",
        CONFIG,
        &[
            ("requireRegistration", "Compatibility"),
            ("checkBrand", "Compatibility"),
            ("checkOwner", "Compatibility"),
            ("acceptWrongId", "Compatibility"),
            ("numericCoercion", "Compatibility"),
            ("truncateUnknown", "StoredValue"),
            ("mutateRejected", "StoredValue"),
            ("loseExtension", "SourceMember"),
            ("inventUnknown", "SourceMember"),
        ],
        None,
    )
    .unwrap();
}
