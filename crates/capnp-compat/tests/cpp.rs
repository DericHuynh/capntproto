#![cfg(target_os = "linux")]
mod common;
#[path = "common/text_assignments.rs"]
mod text_assignments;
#[path = "common/text_scalars.rs"]
mod text_scalars;
#[path = "common/text_wrapping.rs"]
mod text_wrapping;
use capnp::{message, schema_loader::dynamic};
use capnp_compat::{json::JsonCodec, text::TextCodec};
use reproto_test_support::verification::{command, cpp, root, run};
use std::fs;
#[test]
fn codecs_match_pinned_cpp_wire_and_encodings() {
    let build = cpp::build(&[
        "capnp-json",
        "capnpc",
        "capnp-rpc",
        "capnp_tool",
        "capnpc_cpp",
    ])
    .unwrap();
    for name in [
        "compat/byte-stream.capnp",
        "compat/http-over-capnp.capnp",
        "compat/json.capnp",
        "c++.capnp",
        "stream.capnp",
    ] {
        assert_eq!(
            fs::read(root().join("crates/capnp-compat/schema/capnp").join(name)).unwrap(),
            fs::read(root().join("vendor/capnproto/c++/src/capnp").join(name)).unwrap(),
            "pinned schema drift: {name}"
        );
    }
    let logs = root().join("target/verification/compat");
    fs::create_dir_all(&logs).unwrap();
    let binary = logs.join("codecs");
    let generated = logs.join("codec-generated");
    fs::create_dir_all(&generated).unwrap();
    run(
        command(build.join("c++/src/capnp/capnp"))
            .args([
                "compile",
                "-Ivendor/capnproto/c++/src",
                "--src-prefix=crates/capnp-compat/tests",
            ])
            .arg(format!(
                "-o{}:{}",
                build.join("c++/src/capnp/capnpc-c++").display(),
                generated.display()
            ))
            .arg("crates/capnp-compat/tests/compat.capnp"),
        &logs.join("codec-generate.log"),
        0,
    )
    .unwrap();
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "crates/capnp-compat/tests/cpp-codecs.c++",
            ])
            .arg(format!("-I{}", generated.display()))
            .arg(generated.join("compat.capnp.c++"))
            .arg(build.join("c++/src/capnp/libcapnp-rpc.a"))
            .arg(build.join("c++/src/capnp/libcapnp-json.a"))
            .arg(build.join("c++/src/capnp/libcapnpc.a"))
            .arg(build.join("c++/src/capnp/libcapnp.a"))
            .arg(build.join("c++/src/kj/libkj-async.a"))
            .arg(build.join("c++/src/kj/libkj.a"))
            .args(["-pthread", "-o"])
            .arg(&binary),
        &logs.join("build.log"),
        0,
    )
    .unwrap();
    let mut cases = vec![
        (
            "text",
            "Record",
            "plain",
            r#"(i8 = -128, i16 = -32768, i32 = -2147483648, i64 = -9223372036854775808, u8 = 255, u16 = 65535, u32 = 4294967295, u64 = 18446744073709551615, f32 = 1.5, f64 = -inf, text = "a\nb", data = 0x"00ff80", nums = [-1, 42], child = (text = "nested"), children = [(i8 = 7), (flag = true)], lists = [[1, 2], []], choice = second, some = "chosen", details = (amount = 10))"#,
        ),
        (
            "json",
            "Record",
            "plain",
            r#"{"text":"hello","data":[1,2,255],"nums":["-1","9223372036854775807"],"child":{"flag":true},"choice":"second","some":""}"#,
        ),
        ("json", "Record", "nondefault", r#"{}"#),
        (
            "json",
            "Record",
            "plain",
            r#"{"f32":null,"f64":"NaN","i32":"-44","text":null,"some":null}"#,
        ),
        (
            "json",
            "Annotated",
            "annotations",
            r#"{"value":42,"renamed_field":"hello","p_count":9,"p_flag":true,"data":"AP+A","hex":"00ff80","choices":["SECOND","first"],"embedded":{"a":[1,true,null],"b":"now"},"kind":"number"}"#,
        ),
        (
            "json",
            "FlatUnion",
            "annotations",
            r#"{"b_text":"hello","type":"b","b_i32":12}"#,
        ),
    ];
    cases.push(("text","Record","plain",r#"(f32 = 0.10000001, f64 = 1.2345678901234567, child = (f32 = 1e-7, f64 = 1e100), some = "this is a long value that triggers multiline pretty formatting")"#));
    let scalars = text_scalars::accepted();
    cases.extend(
        scalars
            .iter()
            .map(|s| ("text", "Record", "plain", s.as_str())),
    );
    cases.extend(text_wrapping::accepted().into_iter().map(|case| {
        (
            if case.orphan { "text-orphan" } else { "text" },
            case.schema,
            "plain",
            case.input,
        )
    }));
    let pretty_cases = cases
        .clone()
        .into_iter()
        .map(|(mode, name, options, input)| {
            (
                mode,
                name,
                match options {
                    "annotations" => "annotations-pretty",
                    "nondefault" => "nondefault-pretty",
                    _ => "pretty",
                },
                input,
            )
        })
        .collect::<Vec<_>>();
    cases.extend(pretty_cases);
    let schemas = common::schemas();
    for (i, (mode, name, options, input)) in cases.iter().enumerate() {
        let input_path = logs.join(format!("{i}.input"));
        let output = logs.join(format!("{i}.out"));
        fs::write(&input_path, input).unwrap();
        run(
            command(binary.to_str().unwrap())
                .args([mode, name, options])
                .arg(&input_path)
                .arg(&output),
            &logs.join(format!("{i}.log")),
            0,
        )
        .unwrap();
        let expected = fs::read_to_string(&output).unwrap();
        let mut expected = expected.lines();
        let schema = common::schema(&schemas, name);
        let mut message = message::Builder::new_default();
        let mut json = JsonCodec::new();
        if options.contains("annotations") {
            json.handle_by_annotation(schema.clone()).unwrap();
        }
        if options.contains("nondefault") {
            json.has_mode = dynamic::HasMode::NonDefault;
        }
        let mut text = TextCodec::new();
        json.pretty_print = options.contains("pretty");
        text.pretty_print = json.pretty_print;
        let builder = dynamic::Builder::init(message.init_root(), schema.clone()).unwrap();
        if *mode == "json" {
            json.decode(input, builder).unwrap();
        } else if *mode == "text-orphan" {
            let (mut builder, token) = builder.with_orphanage();
            let value = text
                .decode_orphan(
                    input,
                    capnp::schema_loader::Type::Struct(schema.clone()),
                    &mut token.in_struct(&mut builder).unwrap(),
                )
                .unwrap();
            builder.adopt_content(value).unwrap();
        } else {
            text.decode(input, builder).unwrap();
        }
        let wire = common::wire(&message, schema.clone());
        let hex: String = wire.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, expected.next().unwrap(), "case {i} wire");
        let value = dynamic::Value::Struct(
            dynamic::Reader::new(message.get_root_as_reader().unwrap(), schema).unwrap(),
        );
        let expected_json = fs::read_to_string(output.with_extension("out.json")).unwrap();
        assert_eq!(
            json.encode(value.clone()).unwrap(),
            expected_json,
            "case {i} JSON"
        );
        assert_eq!(
            text.encode(value).unwrap(),
            fs::read_to_string(output.with_extension("out.text")).unwrap(),
            "case {i} text"
        );
    }
    let rejected = text_scalars::rejected();
    let rejected: Vec<_> = rejected
        .iter()
        .map(|s| ("Record", s.as_str(), false))
        .chain(
            text_wrapping::rejected()
                .into_iter()
                .map(|case| (case.schema, case.input, case.orphan)),
        )
        .collect();
    for (i, (name, input, orphan)) in rejected.iter().enumerate() {
        let input_path = logs.join(format!("rejected-{i}.input"));
        fs::write(&input_path, input).unwrap();
        let output = command(&binary)
            .args([if *orphan { "text-orphan" } else { "text" }, name, "plain"])
            .arg(&input_path)
            .arg(logs.join(format!("rejected-{i}.out")))
            .output()
            .unwrap();
        fs::write(logs.join(format!("rejected-{i}.log")), &output.stderr).unwrap();
        assert_eq!(output.status.code(), Some(2), "C++ accepted {input}");
        assert!(!output.stderr.is_empty());
        let mut message = message::Builder::new_default();
        let builder =
            dynamic::Builder::init(message.init_root(), common::schema(&schemas, name)).unwrap();
        if *orphan {
            let (mut builder, token) = builder.with_orphanage();
            assert!(
                TextCodec::new()
                    .decode_orphan(
                        input,
                        capnp::schema_loader::Type::Struct(common::schema(&schemas, name)),
                        &mut token.in_struct(&mut builder).unwrap()
                    )
                    .is_err(),
                "Rust accepted {input}"
            );
        } else {
            assert!(
                TextCodec::new().decode(input, builder).is_err(),
                "Rust accepted {input}"
            );
        }
    }
    let assignments = text_assignments::cases();
    for (i, case) in assignments.iter().enumerate() {
        for pretty in [false, true] {
            let label = format!("assignment-{i}-{}", if pretty { "pretty" } else { "plain" });
            let input_path = logs.join(format!("{label}.input"));
            let seed_path = logs.join(format!("{label}.seed"));
            let output = logs.join(format!("{label}.out"));
            fs::write(&input_path, case.input).unwrap();
            fs::write(&seed_path, case.seed).unwrap();
            let mode = match (case.orphan, case.error) {
                (false, false) => "text",
                (false, true) => "text-error",
                (true, false) => "text-orphan",
                (true, true) => "text-orphan-error",
            };
            run(
                command(&binary)
                    .args([mode, case.schema, if pretty { "pretty" } else { "plain" }])
                    .arg(&input_path)
                    .arg(&output)
                    .arg(&seed_path),
                &logs.join(format!("{label}.log")),
                0,
            )
            .unwrap();
            let schema = common::schema(&schemas, case.schema);
            let mut message = message::Builder::new_default();
            let mut builder = dynamic::Builder::init(message.init_root(), schema.clone()).unwrap();
            let text = TextCodec {
                pretty_print: pretty,
            };
            let mut json = JsonCodec::new();
            json.pretty_print = pretty;
            text.decode(case.seed, builder.reborrow()).unwrap();
            let result = if case.orphan {
                let (mut builder, token) = builder.reborrow().with_orphanage();
                text.decode_orphan(
                    case.input,
                    capnp::schema_loader::Type::Struct(schema.clone()),
                    &mut token.in_struct(&mut builder).unwrap(),
                )
                .and_then(|value| builder.adopt_content(value).map_err(|e| e.error))
            } else {
                text.decode(case.input, builder)
            };
            assert_eq!(result.is_err(), case.error, "{label}: {result:?}");
            let hex: String = common::wire(&message, schema.clone())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            let expected = fs::read_to_string(&output).unwrap();
            assert_eq!(hex, expected.lines().next().unwrap(), "{label} wire");
            let value = dynamic::Value::Struct(
                dynamic::Reader::new(message.get_root_as_reader().unwrap(), schema).unwrap(),
            );
            assert_eq!(
                json.encode(value.clone()).unwrap(),
                fs::read_to_string(output.with_extension("out.json")).unwrap(),
                "{label} JSON"
            );
            assert_eq!(
                text.encode(value).unwrap(),
                fs::read_to_string(output.with_extension("out.text")).unwrap(),
                "{label} text"
            );
        }
    }
    fs::write(logs.join("codecs-summary.txt"), format!("{} accepted inputs match canonical wire and exact JSON/text encoding (including compact/pretty pairs); {} shared numeric/type rejections; {} ordered-assignment input/format combinations match resulting wire and encodings ({} failures).\n", cases.len(), rejected.len(), assignments.len() * 2, assignments.iter().filter(|case| case.error).count() * 2)).unwrap();
}
