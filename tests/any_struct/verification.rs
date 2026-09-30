use super::*;
use reproto_test_support::verification::{command, cpp, exploration, root, run};

fn pointer_value(pointer: any_pointer::Reader<'_>) -> u64 {
    match pointer.get_pointer_type().unwrap() {
        PointerType::Null => 0,
        PointerType::List => {
            assert_eq!(pointer.get_as::<capnp::data::Reader>().unwrap(), &[73]);
            1
        }
        PointerType::Capability => {
            let cap: service::Client = pointer.get_as_capability().unwrap();
            let response = futures::executor::block_on(cap.ping_request().send().promise).unwrap();
            assert_eq!(response.get().unwrap().get_value(), 17);
            2
        }
        PointerType::Struct => panic!("unexpected struct"),
    }
}

fn fields(message: &Message) -> [u64; 6] {
    let value = message.value();
    let data = value.get_data_section();
    let pointers = value.get_pointer_section();
    let d = data.len() as u64 / 8;
    let p = u64::from(pointers.len());
    let a = u64::from(data.first().copied().unwrap_or(0));
    let b = u64::from(data.last().copied().unwrap_or(0));
    let x = pointers.try_get(0).map(pointer_value).unwrap_or(0);
    let y = pointers.try_get(1).map(pointer_value).unwrap_or(0);
    // Also check that mutations didn't damage any unmodeled bytes.
    if data.len() > 2 {
        assert!(data[1..data.len() - 1].iter().all(|byte| *byte == 0));
    }
    let size = value.total_size().unwrap();
    assert_eq!(
        size.word_count,
        d + p + u64::from(x == 1) + u64::from(y == 1)
    );
    assert_eq!(size.cap_count, u32::from(x == 2) + u32::from(y == 2));
    [d, p, a, b, x, y]
}

fn snapshot(message: &Message) -> u64 {
    if message.reader().is_null() {
        return 0;
    }
    let [d, p, a, b, x, y] = fields(message);
    100000 + 10000 * d + 1000 * p + 100 * a + 10 * b + 3 * x + y
}

#[test]
fn tlc_any_struct_sections_copy_and_canonicalization_match_cpp() {
    const MODEL: &str = "verification/AnyStruct.tla";
    const CONFIG: &str = include_str!("../../verification/AnyStruct.cfg");
    let paths = exploration::traces(MODEL, "any-struct", CONFIG).unwrap();
    let mut inputs = String::new();
    let mut expected = String::new();
    let mut observations = 0;
    for path in &paths {
        let mut source = Message::default();
        let mut copy = Message::default();
        for state in path {
            let event = state["event"];
            inputs += &format!("{event} ");
            let mut canonical = [0, 0, 0];
            match event {
                1..=9 => {
                    source
                        .builder()
                        .init_as_any_struct(((event - 1) / 3) as u16, ((event - 1) % 3) as u16);
                }
                20 | 21 => {
                    let mut value = source.builder().get_as::<any_struct::Builder>().unwrap();
                    let data = value.get_data_section();
                    if event == 20 {
                        data[0] = 1;
                    } else {
                        *data.last_mut().unwrap() = 2;
                    }
                }
                30..=35 => {
                    let mut value = source.builder().get_as::<any_struct::Builder>().unwrap();
                    let mut pointer = value.get_pointer_section().get(((event - 30) / 3) as u32);
                    match (event - 30) % 3 {
                        0 => pointer.clear(),
                        1 => pointer.set_as::<capnp::data::Owned>(&[73u8][..]).unwrap(),
                        2 => pointer.set_as_capability(server().into_client_hook()),
                        _ => unreachable!(),
                    }
                }
                40 => copy
                    .builder()
                    .set_as::<any_pointer::Owned>(source.value())
                    .unwrap(),
                41 => source.builder().clear(),
                42 => match source.value().canonicalize() {
                    Ok(words) => {
                        let segments = [Word::words_to_bytes(&words)];
                        let reader =
                            message::Reader::new(&segments[..], message::ReaderOptions::new());
                        assert!(reader.is_canonical().unwrap());
                        let value = reader.get_root::<any_struct::Reader>().unwrap();
                        assert_eq!(value.equals(source.value()).unwrap(), Equality::Equal);
                        canonical = [
                            1,
                            value.get_data_section().len() as u64 / 8,
                            u64::from(value.get_pointer_section().len()),
                        ];
                    }
                    Err(_) => canonical[0] = 2,
                },
                _ => panic!("bad event {event}"),
            }
            let [d, p, a, b, x, y] = fields(&source);
            let seen = [
                d,
                p,
                1000 * a + 100 * b + 10 * x + y,
                snapshot(&copy),
                canonical[0],
                canonical[1],
                canonical[2],
            ];
            assert_eq!(
                seen,
                [
                    state["seenD"],
                    state["seenP"],
                    state["seenValue"],
                    state["seenCopy"],
                    state["canon"],
                    state["canonD"],
                    state["canonP"]
                ],
                "{path:?}"
            );
            expected += &format!(
                "{} {} {} {} {} {} {},",
                seen[0], seen[1], seen[2], seen[3], seen[4], seen[5], seen[6]
            );
            observations += 1;
        }
        inputs.push('\n');
        expected.push('\n');
    }
    eprintln!(
        "{observations} Rust/C++ observations across {} edge-prefix scenarios",
        paths.len()
    );
    cpp_replay(&inputs, &expected);
    exploration::controls(
        MODEL,
        "any-struct",
        CONFIG,
        &[
            ("truncateData", "Sections"),
            ("truncatePointers", "Sections"),
            ("swapSlots", "Sections"),
            ("dropCapabilities", "IndependentCopy"),
            ("aliasCopy", "IndependentCopy"),
            ("canonicalCaps", "CanonicalContract"),
            ("canonicalPadding", "CanonicalContract"),
        ],
        None,
    )
    .unwrap();
}

fn cpp_replay(inputs: &str, expected: &str) {
    let build = cpp::build(&["capnpc", "capnp_tool", "capnpc_cpp", "capnp-rpc"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/any-struct-cpp");
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
    let exe = temp.path().join("any-struct");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", temp.path().display()))
            .arg("tests/cpp/any-struct.c++")
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
    let actual = run(command(exe).arg(input), &logs.join("reference.log"), 0).unwrap();
    assert_eq!(actual.lines().count(), expected.lines().count());
    for ((actual, expected), case) in actual.lines().zip(expected.lines()).zip(inputs.lines()) {
        assert_eq!(actual, expected, "{case}");
    }
}
