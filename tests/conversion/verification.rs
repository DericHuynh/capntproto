use super::super::*;
use reproto_test_support::verification::{command, cpp, exploration, root, run};

fn encoded(input: Input) -> (u8, String) {
    match input {
        Input::Signed(v) => (0, v.to_string()),
        Input::Unsigned(v) => (1, v.to_string()),
        Input::Float(v) => (2, v.to_bits().to_string()),
        Input::Float32(v) => (3, v.to_bits().to_string()),
        Input::Bool(v) => (4, u8::from(v).to_string()),
        Input::Void => (5, "0".into()),
        Input::Text(v) => (
            6,
            if v.is_empty() {
                "_".into()
            } else {
                hex(v.as_bytes())
            },
        ),
        Input::Data(v) => (7, if v.is_empty() { "_".into() } else { hex(v) }),
        Input::Enum(v, false) => (8, v.to_string()),
        Input::Enum(v, true) => (9, v.to_string()),
    }
}
#[test]
fn checked_conversions_match_defined_pinned_cpp_cases() {
    let build = cpp::build(&["capnpc", "capnp_tool"]).unwrap();
    let logs = root().join("target/verification/dynamic-conversion-cpp");
    let tmp = tempfile::tempdir().unwrap();
    let exe = tmp.path().join("conversion");
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/dynamic-conversion.c++",
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
            "schemas/conversion.capnp",
        ])
        .output()
        .unwrap();
    assert!(
        request.status.success(),
        "{}",
        String::from_utf8_lossy(&request.stderr)
    );
    let schema_path = tmp.path().join("schema.bin");
    std::fs::write(&schema_path, request.stdout).unwrap();
    let input_path = tmp.path().join("cases.txt");
    let mut inputs = String::new();
    let mut actual = String::new();
    let mut excluded = 0;
    for input in sources() {
        for target in 0..TARGETS.len() {
            // The pinned C++ check rounds MAX to 2^63/2^64 and permits an
            // undefined float->integer cast at those exact upper boundaries.
            // Test Rust's defined rejection natively, not against C++ UB.
            if matches!(input, Input::Float(v) if
                (target==5 && v==9_223_372_036_854_775_808.0) ||
                (target==9 && v==18_446_744_073_709_551_616.0))
            {
                assert!(input.compiled().try_convert(ty(target)).is_err());
                excluded += 1;
                continue;
            }
            let (kind, value) = encoded(input);
            inputs += &format!("{kind} {value} {target}\n");
            actual += &format!("{}\n", normalize(input.compiled().try_convert(ty(target))));
        }
    }
    assert_eq!(excluded, 2);
    std::fs::write(input_path.clone(), inputs).unwrap();
    let reference = run(
        command(exe)
            .arg(schema_path)
            .arg(input_path)
            .arg(schema().get_proto().get_id().to_string())
            .arg(
                enum_schema(conversion_capnp::Foreign::introspect())
                    .get_proto()
                    .get_id()
                    .to_string(),
            ),
        &logs.join("reference.log"),
        0,
    )
    .unwrap();
    assert_eq!(reference, actual);
    eprintln!("{} defined C++ conversion observations; {excluded} undefined C++ boundary cases excluded and tested natively",actual.lines().count());
}

// These representatives cover range, signedness, fractions, non-finite floats,
// enum names/ordinals and mismatches. TLA checks their independent mathematical
// rules; arbitrary IEEE-754 payloads and all widths are native/C++ test scope.
fn model_input(index: u64) -> Input {
    match index {
        0 => Input::Signed(-1),
        1 => Input::Unsigned(0),
        2 => Input::Signed(127),
        3 => Input::Signed(128),
        4 => Input::Unsigned(255),
        5 => Input::Unsigned(256),
        6 => Input::Float(1.0),
        7 => Input::Float(1.5),
        8 => Input::Float(f64::NAN),
        9 => Input::Text("other"),
        10 => Input::Text("missing"),
        11 => Input::Bool(true),
        12 => Input::Unsigned(65535),
        13 => Input::Unsigned(65536),
        14 => Input::Enum(1, false),
        15 => Input::Enum(1, true),
        _ => panic!(),
    }
}
#[test]
fn tlc_conversion_then_assignment_preserves_destinations_on_error() {
    const MODEL: &str = "verification/DynamicConversion.tla";
    const CONFIG: &str = include_str!("../../verification/DynamicConversion.cfg");
    let loader = loader();
    for path in exploration::traces(MODEL, "dynamic-conversion", CONFIG).unwrap() {
        let mut m = message::Builder::new_default();
        let mut r = R::Void; // The conversion result is independent of the destination.
        let mut builder = dynamic_value::Builder::from(m.init_root::<target::Builder>())
            .downcast::<dynamic_struct::Builder>();
        let mut loaded_message = message::Builder::new_default();
        let mut loaded =
            dynamic::Builder::init(loaded_message.init_root(), loaded_schema(&loader)).unwrap();
        let mut stored = 0;
        let mut active = 0;
        for state in path {
            let index = [2, 6, 12][usize::try_from(state["target"]).unwrap()];
            let input = model_input(state["input"]);
            let name = TARGETS[index];
            let field = builder.get_schema().get_field_by_name(name).unwrap();
            let loaded_field = loaded.schema().field(name).unwrap();
            let lt = loaded_field.get_type().unwrap();
            let before = builder
                .reborrow_as_reader()
                .which()
                .unwrap()
                .unwrap()
                .get_index();
            let result = input.compiled().try_convert(field.get_type());
            let lv = input.loaded(&loader).try_convert(&lt);
            assert_eq!(result.is_ok(), state["success"] == 1, "{state:?}");
            assert_eq!(normalize(result.clone()), normalize_loaded(lv.clone()));
            if let Ok(value) = result {
                stored = match &value {
                    R::Int8(v) => u64::try_from(i64::from(*v) + 1).unwrap(),
                    R::UInt8(v) => u64::from(*v) + 1,
                    R::Enum(v) => u64::from(v.get_value()) + 1,
                    _ => panic!(),
                };
                active = state["target"] + 1;
                builder.set(field, value.clone()).unwrap();
                loaded.set(loaded_field, lv.unwrap()).unwrap();
                r = value;
            } else {
                assert_eq!(
                    builder
                        .reborrow_as_reader()
                        .which()
                        .unwrap()
                        .unwrap()
                        .get_index(),
                    before
                );
                assert_eq!(loaded.as_reader().which().unwrap().unwrap().index(), before);
            }
            assert_eq!(
                (active, stored),
                (state["active"], state["stored"]),
                "{state:?}"
            );
            if active != 0 {
                let name = TARGETS[[2, 6, 12][usize::try_from(active - 1).unwrap()]];
                assert_eq!(
                    normalize(builder.reborrow_as_reader().get_named(name)),
                    normalize(Ok(r.clone()))
                );
                assert_eq!(
                    normalize_loaded(loaded.as_reader().get_named(name)),
                    normalize(Ok(r.clone()))
                );
            }
        }
    }
    exploration::controls(
        MODEL,
        "dynamic-conversion",
        CONFIG,
        &[
            ("wrapUnsigned", "CorrectConversion"),
            ("truncateFraction", "CorrectConversion"),
            ("floatEnum", "CorrectConversion"),
            ("foreignEnum", "CorrectConversion"),
            ("selectOnError", "FailurePreservesDestination"),
        ],
        None,
    )
    .unwrap();
}
