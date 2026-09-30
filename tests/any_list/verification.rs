use super::*;
use reproto_test_support::verification::{command, cpp, exploration, root, run};

fn pointer_value(pointer: any_pointer::Reader<'_>) -> u64 {
    match pointer.get_pointer_type().unwrap() {
        any_pointer::PointerType::Null => 0,
        any_pointer::PointerType::List => {
            assert_eq!(pointer.get_as::<capnp::data::Reader>().unwrap(), &[73]);
            1
        }
        any_pointer::PointerType::Capability => {
            ping(pointer);
            2
        }
        _ => panic!(),
    }
}
fn fields(message: &Message) -> [u64; 4] {
    let list = message.list();
    if list.is_empty() {
        return [0; 4];
    }
    if list.get_element_size() == ElementSize::Bit {
        let bits = list.get_as::<capnp::primitive_list::Owned<bool>>().unwrap();
        return [
            u64::from(bits.get(0)),
            u64::from(bits.get(bits.len() - 1)),
            0,
            0,
        ];
    }
    let structs = list.get_as_struct_list().unwrap();
    let first = structs.get(0).unwrap();
    let last = structs.get(structs.len() - 1).unwrap();
    let a = u64::from(first.get_data_section().first().copied().unwrap_or(0));
    let b = u64::from(last.get_data_section().last().copied().unwrap_or(0));
    let x = first
        .get_pointer_section()
        .try_get(0)
        .map(pointer_value)
        .unwrap_or(0);
    let pointers = last.get_pointer_section();
    let y = if pointers.is_empty() {
        0
    } else {
        pointer_value(pointers.get(pointers.len() - 1))
    };
    // Every byte/slot not targeted by the model must still be zero/null.
    for i in 0..structs.len() {
        let value = structs.get(i).unwrap();
        for (j, byte) in value.get_data_section().iter().enumerate() {
            if !(i == 0 && j == 0
                || i == structs.len() - 1 && j == value.get_data_section().len() - 1)
            {
                assert_eq!(*byte, 0);
            }
        }
        for j in 0..value.get_pointer_section().len() {
            if !(i == 0 && j == 0
                || i == structs.len() - 1 && j == value.get_pointer_section().len() - 1)
            {
                assert!(value.get_pointer_section().get(j).is_null());
            }
        }
    }
    [a, b, x, y]
}
fn snapshot(message: &Message, kind: u64) -> u64 {
    if message.reader().is_null() {
        return 0;
    }
    let [a, b, x, y] = fields(message);
    10000000
        + 1000000 * kind
        + 100000 * u64::from(message.list().len())
        + 10000 * a
        + 1000 * b
        + 10 * x
        + y
}

#[test]
fn tlc_any_list_layout_mutation_and_copy_match_cpp() {
    const MODEL: &str = "verification/AnyList.tla";
    const CONFIG: &str = include_str!("../../verification/AnyList.cfg");
    let paths = exploration::traces(MODEL, "any-list", CONFIG).unwrap();
    let mut inputs = String::new();
    let mut expected = String::new();
    let mut observations = 0;
    for path in &paths {
        let mut source = Message::default();
        let mut copy = Message::default();
        let mut kind = 0;
        let mut copied_kind = 0;
        for state in path {
            let event = state["event"];
            inputs += &format!("{event} ");
            let mut raw = [0, 0];
            let mut projected = 0;
            let mut bit_cast = 0;
            let mut bit_read = 0;
            match event {
                1..=36 => {
                    kind = (event - 1) / 3;
                    allocate(&mut source, kind, ((event - 1) % 3) as u32);
                }
                100 | 101 => {
                    let mut list = source.builder().get_as::<any_list::Builder>().unwrap();
                    let index = if event == 100 { 0 } else { list.len() - 1 };
                    if kind == 1 {
                        list.get_as::<capnp::primitive_list::Owned<bool>>()
                            .unwrap()
                            .set(index, true);
                    } else {
                        let mut value = list.reborrow().get_as_struct_list().unwrap().get(index);
                        let data = value.get_data_section();
                        if event == 100 {
                            data[0] = 1;
                        } else {
                            *data.last_mut().unwrap() = 1;
                        }
                    }
                }
                200..=205 => {
                    let mut list = source
                        .builder()
                        .get_as::<any_struct_list::Builder>()
                        .unwrap();
                    let index = if event < 203 { 0 } else { list.len() - 1 };
                    let mut value = list.reborrow().get(index);
                    let pointers = value.get_pointer_section();
                    let slot = if event < 203 { 0 } else { pointers.len() - 1 };
                    let mut pointer = pointers.get(slot);
                    match (event - 200) % 3 {
                        0 => pointer.clear(),
                        1 => pointer.set_as::<capnp::data::Owned>(&[73u8][..]).unwrap(),
                        2 => pointer.set_as_capability(server().into_client_hook()),
                        _ => unreachable!(),
                    }
                }
                300 => {
                    copy.builder()
                        .set_as::<any_pointer::Owned>(source.list())
                        .unwrap();
                    copied_kind = kind;
                }
                301 => {
                    source.builder().clear();
                    kind = 0;
                }
                302 => {
                    raw = match source.list().get_raw_bytes() {
                        Ok(bytes) => [1, bytes.len() as u64],
                        Err(_) => [2, 0],
                    };
                }
                303 => {
                    let mut list = source
                        .builder()
                        .get_as::<capnp::any_pointer_list::Builder>()
                        .unwrap();
                    list.reborrow()
                        .get(0)
                        .set_as::<capnp::data::Owned>(&[73u8][..])
                        .unwrap();
                    let reader = list.into_reader();
                    projected = pointer_value(reader.get(0));
                    let erased = any_list::Reader::from_reader(reader);
                    assert_eq!(erased.get_element_size(), encoding(kind));
                    assert_eq!(
                        erased.total_size().unwrap().word_count,
                        source.list().total_size().unwrap().word_count
                    );
                }
                304 => {
                    let readable = source
                        .reader()
                        .get_as::<capnp::primitive_list::Reader<bool>>()
                        .is_ok();
                    let writable = source
                        .builder()
                        .get_as::<capnp::primitive_list::Builder<bool>>()
                        .is_ok();
                    bit_read = if readable { 1 } else { 2 };
                    bit_cast = if writable { 1 } else { 2 };
                }
                _ => panic!("bad event {event}"),
            }
            let list = source.list();
            let [a, b, x, y] = fields(&source);
            let size = list.total_size().unwrap();
            let seen = [
                list.get_element_size() as u64,
                u64::from(list.len()),
                1000 * a + 100 * b + 10 * x + y,
                snapshot(&copy, copied_kind),
                size.word_count,
                u64::from(size.cap_count),
                raw[0],
                raw[1],
                projected,
                bit_cast,
                bit_read,
            ];
            let keys = [
                "seenKind",
                "seenCount",
                "seenValue",
                "seenCopy",
                "seenWords",
                "seenCaps",
                "raw",
                "rawBytes",
                "projected",
                "bitCast",
                "bitRead",
            ];
            for (actual, key) in seen.into_iter().zip(keys) {
                assert_eq!(actual, state[key], "{key}: {path:?}");
            }
            expected += &format!(
                "{} {} {} {} {} {} {} {} {} {} {},",
                seen[0],
                seen[1],
                seen[2],
                seen[3],
                seen[4],
                seen[5],
                seen[6],
                seen[7],
                seen[8],
                seen[9],
                seen[10]
            );
            observations += 1;
        }
        inputs.push('\n');
        expected.push('\n');
    }
    eprintln!(
        "{observations} Rust/C++ observations across {} edge-prefix traces",
        paths.len()
    );
    cpp_replay(&inputs, &expected);
    exploration::controls(
        MODEL,
        "any-list",
        CONFIG,
        &[
            ("truncateCount", "Layout"),
            ("wrongStride", "Layout"),
            ("aliasCopy", "IndependentCopy"),
            ("dropCaps", "IndependentCopy"),
            ("dropTag", "Size"),
            ("hideCaps", "Size"),
            ("exposePointers", "RawContract"),
            ("truncateBits", "RawContract"),
            ("doubleOffset", "ProjectionContract"),
            ("allowNonBit", "BitContract"),
        ],
        None,
    )
    .unwrap();
}

fn cpp_replay(inputs: &str, expected: &str) {
    let build = cpp::build(&["capnpc", "capnp_tool", "capnpc_cpp", "capnp-rpc"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/any-list-cpp");
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
    let exe = temp.path().join("any-list");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", temp.path().display()))
            .arg("tests/cpp/any-list.c++")
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
    let discrepancy = run(
        command(&exe).arg("--projection-regression"),
        &logs.join("projection-regression.log"),
        0,
    )
    .unwrap();
    assert_eq!(
        discrepancy.trim(),
        "1",
        "pinned C++ double-offset regression"
    );
    let actual = run(command(exe).arg(input), &logs.join("reference.log"), 0).unwrap();
    assert_eq!(actual.lines().count(), expected.lines().count());
    for ((a, e), case) in actual.lines().zip(expected.lines()).zip(inputs.lines()) {
        assert_eq!(a, e, "{case}");
    }
}
