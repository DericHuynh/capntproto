use super::super::*;
use capntproto_test_support::verification::{command, cpp, exploration, root, run};

#[test]
fn tlc_union_presence_traces_match_compiled_and_loaded_rust() {
    const MODEL: &str = "verification/DynamicFieldPresence.tla";
    const CONFIG: &str = include_str!("../../verification/DynamicFieldPresence.cfg");
    let loader = loader();
    let initial = frame(|r| r.get_body().set_note("default"));
    let pointer = pointer_offset(body_schema(), "note");
    let number = 16 + usize::try_from(slot(body_schema(), "number")).unwrap() * 4;
    let tag = tag_offset();
    for path in exploration::traces(MODEL, "field-presence", CONFIG).unwrap() {
        let mut bytes = initial.clone();
        bytes[pointer..pointer + 8].fill(0);
        bytes[tag..tag + 2].fill(0);
        for state in path {
            match state["event"] {
                1 => bytes[number..number + 4]
                    .copy_from_slice(&u32::try_from(state["bits"]).unwrap().to_le_bytes()),
                2 => {
                    if state["pointer"] == 0 {
                        bytes[pointer..pointer + 8].fill(0);
                    } else {
                        bytes[pointer..pointer + 8].copy_from_slice(&initial[pointer..pointer + 8]);
                    }
                }
                3 => bytes[tag..tag + 2]
                    .copy_from_slice(&u16::try_from(state["tag"]).unwrap().to_le_bytes()),
                4 => {
                    let m = read(&bytes);
                    let r = compiled(&m);
                    let l =
                        loaded::Reader::new(m.get_root().unwrap(), loaded_schema(&loader)).unwrap();
                    let name = ["none", "number", "note", "details", "body"]
                        [usize::try_from(state["field"]).unwrap()];
                    let mode = MODES[usize::try_from(state["mode"]).unwrap()];
                    let (r, l) = if name == "body" {
                        (r, l)
                    } else {
                        let loaded::Value::Struct(group) = l.get_named("body").unwrap() else {
                            panic!()
                        };
                        (r.get_named("body").unwrap().downcast(), group)
                    };
                    let expected = state["result"] == 1;
                    assert_eq!(
                        r.has_named_with_mode(name, mode).unwrap(),
                        expected,
                        "{state:?}"
                    );
                    assert_eq!(
                        l.has_named_with_mode(name, mode).unwrap(),
                        expected,
                        "{state:?}"
                    );
                }
                _ => panic!("{state:?}"),
            }
            // Query steps must leave all wire bytes untouched. Mutation steps
            // above change only their designated field, preserving stale slots.
            assert_eq!(
                u32::from_le_bytes(bytes[number..number + 4].try_into().unwrap()),
                u32::try_from(state["bits"]).unwrap()
            );
            assert_eq!(
                bytes[pointer..pointer + 8].iter().any(|b| *b != 0),
                state["pointer"] != 0
            );
            assert_eq!(
                u16::from_le_bytes(bytes[tag..tag + 2].try_into().unwrap()),
                u16::try_from(state["tag"]).unwrap()
            );
        }
    }
    exploration::controls(
        MODEL,
        "field-presence",
        CONFIG,
        &[
            ("ignoreUnion", "AccuratePresence"),
            ("decodedZero", "AccuratePresence"),
            ("pointerContents", "AccuratePresence"),
            ("emptyGroup", "AccuratePresence"),
        ],
        None,
    )
    .unwrap();
}

#[test]
fn presence_observations_match_pinned_cpp() {
    let build = cpp::build(&["capnpc", "capnp_tool"]).unwrap();
    let logs = root().join("target/verification/field-presence-cpp");
    let tmp = tempfile::tempdir().unwrap();
    let exe = tmp.path().join("presence");
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/field-presence.c++",
            ])
            .arg(build.join("c++/src/capnp/libcapnpc.a"))
            .arg(build.join("c++/src/capnp/libcapnp.a"))
            .arg(build.join("c++/src/kj/libkj.a"))
            .args(["-pthread", "-o"])
            .arg(&exe),
        &logs.join("compile.log"),
        0,
    )
    .unwrap();
    let request = command(build.join("c++/src/capnp/capnp"))
        .args([
            "compile",
            "-o-",
            "-Ivendor/capnproto/c++/src",
            "--src-prefix=schemas",
            "schemas/presence.capnp",
        ])
        .output()
        .unwrap();
    assert!(
        request.status.success(),
        "{}",
        String::from_utf8_lossy(&request.stderr)
    );
    let request_path = tmp.path().join("schema.bin");
    std::fs::write(&request_path, request.stdout).unwrap();
    let mut frames: Vec<_> = cases().into_iter().map(|c| c.bytes).collect();
    for bits in 0..2u32 {
        for pointer in [false, true] {
            for tag in 0..5u16 {
                let mut bytes = frame(|r| {
                    let mut body = r.get_body();
                    body.set_number(42 ^ bits);
                    if pointer {
                        body.set_note("default");
                    }
                });
                bytes[tag_offset()..tag_offset() + 2].copy_from_slice(&tag.to_le_bytes());
                frames.push(bytes);
            }
        }
    }
    let frames_path = tmp.path().join("frames.bin");
    std::fs::write(&frames_path, frames.concat()).unwrap();
    let reference = run(
        command(exe)
            .arg(request_path)
            .arg(frames_path)
            .arg(schema().get_proto().get_id().to_string())
            .arg(frames.len().to_string()),
        &logs.join("reference.log"),
        0,
    )
    .unwrap();
    let loader = loader();
    let mut actual = String::new();
    for (index, bytes) in frames.iter().enumerate() {
        let m = read(bytes);
        let observations = observe(compiled(&m));
        assert_eq!(
            observations,
            observe_loaded(
                loaded::Reader::new(m.get_root().unwrap(), loaded_schema(&loader)).unwrap()
            )
        );
        for (field, a, b) in observations {
            actual += &format!("{index} {field} {} {}\n", u8::from(a), u8::from(b));
        }
    }
    assert_eq!(reference, actual);
    eprintln!(
        "{} C++ field observations across {} messages",
        actual.lines().count(),
        frames.len()
    );
}
